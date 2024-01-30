use core::panic;
use std::{env, fs::create_dir_all, path::PathBuf};

use chrono::{DateTime, Duration, Utc};
use clap::{arg, Command};
use humantime::format_duration;
use shellexpand;
use sqlite::{Connection, State};
use tabled::{builder::Builder, settings::Style};

// A mission is a task with a name, start_date and end_date
// A mission is considered `ongoing` whenever it has no `end_date`
#[derive(Debug)]
struct Mission {
    name: String,
    start_date: DateTime<Utc>,
    end_date: Option<DateTime<Utc>>,
}

impl Mission {
    fn new(name: String, start_date: DateTime<Utc>) -> Self {
        Self {
            name,
            start_date,
            end_date: None,
        }
    }

    fn elapsed_time(&self) -> Duration {
        let duration = match self.end_date {
            Some(end_date) => end_date - self.start_date,
            None => Utc::now() - self.start_date,
        };
        Duration::seconds(duration.num_seconds())
    }
}

// Get the path to the db file
const DEFAULT_DB_PATH: &str = "~/.local/share/tempo/tempo.db";

// TODO: Add export command to generate a CSV of the data range
fn main() {
    ensure_db_path();

    let db = Connection::open(get_db_path()).expect("Failed to open the database");

    // Create table if it does not exist
    db.execute("CREATE TABLE IF NOT EXISTS missions (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, start_date TEXT NOT NULL, end_date TEXT)").unwrap();

    let cmd = Command::new("tempo")
        .about("Personal time tracking utility")
        .version("0.1.0")
        .subcommand_required(true)
        .subcommand(
            Command::new("start")
                .about("Start a new mission")
                .arg(arg!(<NAME> "The name of the mission"))
                .arg_required_else_help(true),
        )
        .subcommand(Command::new("status").about("Show the current mission status"))
        .subcommand(Command::new("stop").about("Stop all ongoing missions"))
        .subcommand(Command::new("resume").about("Resume the latest stopped mission"))
        .subcommand(
            Command::new("ls")
                .about("List the missions")
                .arg(arg!(--from <FROM> "The start of the selection date range"))
                .arg_required_else_help(false),
        )
        .subcommand(Command::new("info").about("Print system information"));

    let matches = cmd.get_matches();

    match matches.subcommand() {
        Some(("start", arg_matches)) => {
            if let Some(name) = arg_matches.get_one::<String>("NAME") {
                let mission = start_new_mission(&name, &db);
                println!("New mission started: {}", mission.name);
            }
        }
        Some(("status", _)) => {
            print_status(&db);
        }
        Some(("stop", _)) => {
            stop_active_missions(&db);
            println!("All ongoing missions have been stopped");
        }
        Some(("resume", _)) => {
            resume_latest_mission(&db);
            println!("Last mission has been resumed if there was any");
        }
        Some(("ls", arg_matches)) => {
            if let Some(from) = arg_matches.get_one::<String>("from") {
                print_report(&db, &from);
            } else {
                list_missions(&db);
            }
        }
        Some(("info", _)) => {
            print_info(&db);
        }
        _ => unreachable!(),
    }
}

// Get the path to the DB from env or the default one
fn get_db_path() -> String {
    match env::var("TEMPO_DB_PATH") {
        Ok(value) => value,
        Err(_) => shellexpand::full(DEFAULT_DB_PATH).unwrap().to_string(),
    }
}

// Make sure that the path to the db file exists
fn ensure_db_path() {
    let path = PathBuf::from(get_db_path());
    let dir = path.parent().unwrap();
    create_dir_all(dir).unwrap();
}

// Starts a new mission
// Stops all the active missions before hand so theres anly one running
fn start_new_mission(name: &String, db: &Connection) -> Mission {
    stop_active_missions(&db);

    let mission = Mission::new(name.to_string(), Utc::now());

    let mut stmt = db
        .prepare("INSERT INTO missions (name, start_date) VALUES (:name, :start_date)")
        .unwrap();

    stmt.bind((":name", mission.name.as_str())).unwrap();
    stmt.bind((":start_date", mission.start_date.to_rfc3339().as_str()))
        .unwrap();

    stmt.next().expect("Failed to insert mission into db");

    mission
}

// Prints out the latest active mission
fn print_status(db: &Connection) {
    let mut stmt = db
        .prepare("SELECT * FROM missions WHERE end_date IS NULL ORDER BY start_date DESC LIMIT 1")
        .unwrap();

    if let Ok(State::Row) = stmt.next() {
        let name = stmt.read::<String, _>("name").unwrap();
        let start_date =
            DateTime::parse_from_rfc3339(stmt.read::<String, _>("start_date").unwrap().as_str())
                .unwrap()
                .with_timezone(&Utc);
        let mission = Mission::new(name, start_date);
        println!(
            "{} ({})",
            mission.name,
            format_duration(mission.elapsed_time().to_std().unwrap())
        );
    } else {
        println!("No active mission");
    }
}

fn stop_active_missions(db: &Connection) {
    let mut stmt = db
        .prepare("UPDATE missions SET end_date = :end_date WHERE end_date IS NULL")
        .unwrap();

    stmt.bind((":end_date", Utc::now().to_rfc3339().as_str()))
        .unwrap();

    stmt.next().expect("Failed to stop the missions");
}

// TODO: Add an option "-n" to limit the rows
fn list_missions(db: &Connection) {
    let mut stmt = db
        .prepare("SELECT * from missions ORDER BY id DESC LIMIT 10")
        .unwrap();

    let mut builder = Builder::default();
    builder.set_header(["", "Name", "Started At", "Ended At", "Duration"]);

    while let Ok(State::Row) = stmt.next() {
        let name = stmt.read::<String, _>("name").unwrap();
        let start_date =
            DateTime::parse_from_rfc3339(stmt.read::<String, _>("start_date").unwrap().as_str())
                .unwrap()
                .with_timezone(&Utc);

        let end_date: Option<DateTime<Utc>> =
            match stmt.read::<Option<String>, _>("end_date").unwrap() {
                Some(end_date_str) => Some(
                    DateTime::parse_from_rfc3339(&end_date_str)
                        .unwrap()
                        .with_timezone(&Utc),
                ),
                None => None,
            };

        let mut mission = Mission::new(name, start_date);
        mission.end_date = end_date;

        let formatted_end_date = match mission.end_date {
            Some(date) => date.format("%d/%m/%Y %H:%M:%S").to_string(),
            None => String::new(),
        };

        builder.push_record([
            if end_date.is_none() { "⏺" } else { "" },
            mission.name.as_str(),
            mission
                .start_date
                .format("%d/%m/%Y %H:%M:%S")
                .to_string()
                .as_str(),
            formatted_end_date.as_str(),
            format_duration(mission.elapsed_time().to_std().unwrap())
                .to_string()
                .as_str(),
        ]);
    }

    let mut table = builder.build();
    table.with(Style::rounded());

    println!("{}", table);
}

fn resume_latest_mission(db: &Connection) {
    db.execute(
        "UPDATE missions SET end_date = NULL WHERE id IN (SELECT id FROM missions ORDER BY start_date DESC LIMIT 1)",
    )
    .expect("Could not resume the latest mission");
}

fn print_report(db: &Connection, from: &String) {
    // TODO: Get a slice of missions for the given range
    // TODO: Accept strings as date range (e.g last month, yesterday, ...)
    // let mut stmt = db.prepare("SELECT * FROM missions WHERE start_date");

    let now = Utc::now().timestamp();
    let tz = timelib::Timezone::parse("UTC").unwrap();
    let ts = timelib::strtotime(from.as_str(), Some(now), &tz).unwrap();
    let from_date = DateTime::from_timestamp(ts, 0);

    if let Some(date) = from_date {
        if now <= ts {
            panic!("The date range should start from a past date");
        }

        let mut stmt = db
            .prepare("SELECT * FROM missions WHERE start_date >= :start_date OR end_date IS NULL ORDER BY start_date DESC")
            .unwrap();

        stmt.bind((":start_date", date.to_rfc3339().to_string().as_str()))
            .unwrap();

        // TODO: Massive duplicate from the list action
        let mut builder = Builder::default();
        builder.set_header(["", "Name", "Started At", "Ended At", "Duration"]);

        while let Ok(State::Row) = stmt.next() {
            let name = stmt.read::<String, _>("name").unwrap();
            let start_date = DateTime::parse_from_rfc3339(
                stmt.read::<String, _>("start_date").unwrap().as_str(),
            )
            .unwrap()
            .with_timezone(&Utc);

            let end_date: Option<DateTime<Utc>> =
                match stmt.read::<Option<String>, _>("end_date").unwrap() {
                    Some(end_date_str) => Some(
                        DateTime::parse_from_rfc3339(&end_date_str)
                            .unwrap()
                            .with_timezone(&Utc),
                    ),
                    None => None,
                };

            let mut mission = Mission::new(name, start_date);
            mission.end_date = end_date;

            let formatted_end_date = match mission.end_date {
                Some(date) => date.format("%d/%m/%Y %H:%M:%S").to_string(),
                None => String::new(),
            };

            builder.push_record([
                if end_date.is_none() { "⏺" } else { "" },
                mission.name.as_str(),
                mission
                    .start_date
                    .format("%d/%m/%Y %H:%M:%S")
                    .to_string()
                    .as_str(),
                formatted_end_date.as_str(),
                format_duration(mission.elapsed_time().to_std().unwrap())
                    .to_string()
                    .as_str(),
            ]);
        }

        if builder.count_rows() > 0 {
            let mut table = builder.build();
            table.with(Style::rounded());
            println!("{}", table);
        } else {
            println!("Could not find any missions for the provided time range");
        }
    }
}

fn print_info(db: &Connection) {
    println!("Database:");
    println!(" Path: {}", get_db_path());
    println!();

    // TODO: Use count(*)
    let count = db
        .prepare("SELECT id FROM missions")
        .unwrap()
        .into_iter()
        .count();

    println!("Missions: {}", count);

    let active = db
        .prepare("SELECT id FROM missions where end_date IS NULL")
        .unwrap()
        .into_iter()
        .count();

    println!(" Running: {}", active);

    let finished = db
        .prepare("SELECT id FROM missions where end_date IS NOT NULL")
        .unwrap()
        .into_iter()
        .count();

    println!(" Finished: {}", finished);
}
