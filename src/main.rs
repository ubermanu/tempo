use chrono::{DateTime, Duration, Utc};
use clap::{arg, Command};
use humantime::format_duration;
use sqlite::{Connection, State};

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

    fn time_spent(&self) -> Duration {
        match self.end_date {
            Some(end_date) => end_date - self.start_date,
            None => Utc::now() - self.start_date,
        }
    }
}

fn main() {
    let db = Connection::open("tempo.db").expect("Failed to open the database");

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
        .subcommand(Command::new("ls").about("List the missions"))
        .subcommand(Command::new("resume").about("Resume the latest stopped mission"));

    let matches = cmd.get_matches();

    match matches.subcommand() {
        Some(("start", arg_matches)) => {
            if let Some(name) = arg_matches.get_one::<String>("NAME") {
                start_new_mission(&name, &db);
            }
        }
        Some(("status", _)) => {
            print_status(&db);
        }
        Some(("stop", _)) => {
            stop_active_missions(&db);
        }
        Some(("ls", _)) => {
            list_missions(&db);
        }
        Some(("resume", _)) => {
            resume_latest_mission(&db);
        }
        _ => unreachable!(),
    }
}

// Starts a new mission
fn start_new_mission(name: &String, db: &Connection) {
    let mission = Mission::new(name.to_string(), Utc::now());

    let mut stmt = db
        .prepare("INSERT INTO missions (name, start_date) VALUES (:name, :start_date)")
        .unwrap();

    stmt.bind((":name", mission.name.as_str())).unwrap();
    stmt.bind((":start_date", mission.start_date.to_rfc3339().as_str()))
        .unwrap();

    stmt.next().expect("Failed to insert mission into db");

    println!("New mission started: {}", mission.name);
}

// Prints out the latest active mission
fn print_status(db: &Connection) {
    let mut stmt = db
        .prepare("SELECT * FROM missions WHERE end_date IS NULL LIMIT 1")
        .unwrap();

    if let Ok(State::Row) = stmt.next() {
        let name = stmt.read::<String, _>("name").unwrap();
        let start_date =
            DateTime::parse_from_rfc3339(stmt.read::<String, _>("start_date").unwrap().as_str())
                .unwrap()
                .with_timezone(&Utc);
        let mission = Mission::new(name, start_date);
        println!(
            "Active mission: {} -> {}",
            mission.name,
            format_duration(mission.time_spent().to_std().unwrap())
        );
    } else {
        println!("No active missions");
    }
}

fn stop_active_missions(db: &Connection) {
    let mut stmt = db
        .prepare("UPDATE missions SET end_date = :end_date WHERE end_date IS NULL")
        .unwrap();

    stmt.bind((":end_date", Utc::now().to_rfc3339().as_str()))
        .unwrap();

    stmt.next().expect("Failed to stop the missions");

    println!("All ongoing missions have been stopped");
}

fn list_missions(db: &Connection) {
    let mut stmt = db.prepare("SELECT * from missions").unwrap();

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

        println!("{:?}", mission);
    }
}

fn resume_latest_mission(db: &Connection) {
    db.execute(
        "UPDATE missions SET end_date = NULL WHERE id IN (SELECT id FROM missions ORDER BY start_date DESC LIMIT 1)",
    )
    .expect("Could not resume the latest mission");

    println!("Last mission has been resumed if there was any");
}
