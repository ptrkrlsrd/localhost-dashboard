use std::process::Command;

struct Process {
    name: String,
    port: String,
}

const STATUS_COLUMN_NR: usize = 10;
const LISTEN_STATUS: &str = "LISTEN";

fn line_to_words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

fn main() {
    let output = Command::new("lsof")
        .arg("-i")
        .arg("-P")
        .arg("-n")
        .output()
        .expect("failed to execute process");

    let command_output = output.stdout;
    let command_str = match String::from_utf8(command_output) {
        Ok(result) => result,
        Err(e) => panic!("{}", e),
    };


    let lines = command_str.lines()
        .map(|x| line_to_words(x))
        .filter(|i| i.len() == STATUS_COLUMN_NR)
        .filter(|i| i.last().unwrap().contains(LISTEN_STATUS));

    for line in lines {
        let process = Process {
            name: line[0].to_string(),
            port: line[8].to_string(),
        };

        println!("{} - {}", process.name, process.port);
    }

    assert!(output.status.success());
}
