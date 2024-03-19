#[macro_use]
extern crate rocket;

use handlebars::Handlebars;
use rocket::response::content;
use rocket::response::content::Html;
use rocket::State;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

#[derive(Clone, Serialize, Deserialize, Debug)]
struct ProcessState {
    processes: Vec<Process>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct Process {
    name: String,
    addr: String,
}

impl Process {
    fn new(name: &str, addr: &str) -> Result<Process, String> {
        if name.is_empty() || addr.is_empty() {
            return Err("Name or address cannot be empty".to_string());
        }
        Ok(Process {
            name: name.to_string(),
            addr: addr.to_string().replace("*", "127.0.0.1"),
        })
    }
}

const STATUS_COLUMN_NR: usize = 10;
const LISTEN_STATUS: &str = "LISTEN";

fn line_to_words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

fn run_lsof_command() -> String {
    let output = Command::new("lsof")
        .arg("-i")
        .arg("-P")
        .arg("-n")
        .output()
        .expect("failed to execute process");

    let command_output = output.stdout;
    let command_str = match String::from_utf8(command_output) {
        Ok(result) => result,
        Err(e) => panic!("Failed to encode utf8 string with error: {}", e),
    };

    return command_str;
}

fn filter_command_output(command_str: String) -> Vec<Vec<String>> {
    return command_str
        .lines()
        .map(|a| line_to_words(a))
        .filter(|b| b.len() == STATUS_COLUMN_NR)
        .filter(|c| c.last().unwrap().contains(LISTEN_STATUS))
        .collect();
}

fn get_processes() -> Vec<Process> {
    let command_str = run_lsof_command();
    let lines = filter_command_output(command_str);
    let processes = lines
        .into_iter()
        .map(|i| Process::new(&i[0], &i[8]))
        .filter_map(Result::ok)
        .collect::<Vec<Process>>();

    return processes;
}

#[get("/")]
fn index(handlebars: &State<Handlebars>) -> Html<String> {
    let processes = get_processes();

    let process_map = HashMap::from([("processes", processes)]);
    let rendered_html = handlebars.render("dashboard", &process_map).unwrap();

    content::Html(rendered_html)
}

#[rocket::main]
async fn main() {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars
        .register_template_string("dashboard", include_str!("templates/index.hbs"))
        .unwrap();

    let _result = rocket::build()
        .mount("/", routes![index])
        .manage(handlebars)
        .launch()
        .await;
}
