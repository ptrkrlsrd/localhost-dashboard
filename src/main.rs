#[macro_use] extern crate rocket;

use rocket::State;
use std::process::Command;
use std::collections::HashMap;
use rocket::response::content;
use rocket::response::content::Html;
use handlebars::Handlebars;
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
struct ProcessState {
    processes: Vec<Process>
}


#[derive(Clone, Serialize, Deserialize, Debug)]
struct Process {
    name: String,
    port: String,
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
    return command_str.lines()
            .map(|a| line_to_words(a))
            .filter(|b| b.len() == STATUS_COLUMN_NR)
            .filter(|c| c.last().unwrap().contains(LISTEN_STATUS))
        .collect();
}

fn get_processes() -> Vec<Process> {
    let command_str = run_lsof_command();
    let lines = filter_command_output(command_str);
    let processes = lines.into_iter().map(|i| {
        let process = Process {
            name: i[0].to_string(),
            port: i[8].to_string(),
        };
        process
    }).collect::<Vec<Process>>();

    return processes;
}

#[get("/")]
fn index(handlebars: &State<Handlebars>) -> Html<String> {
    let processes = get_processes();

    let mut data = HashMap::new();
    data.insert("processes", processes);

    let rendered_html = handlebars.render("dashboard", &data).unwrap();
    content::Html(rendered_html)
}

#[rocket::main]
async fn main() {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars.register_template_string("dashboard", include_str!("templates/index.hbs")).unwrap();

    let _result = rocket::build()
        .mount("/", routes![index])
        .manage(handlebars)
        .launch()
        .await;
}
