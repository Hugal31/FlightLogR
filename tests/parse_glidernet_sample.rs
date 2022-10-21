use std::fs::File;
use std::io::{BufRead, BufReader};

use flightlogr::aprs::{
    parsing::{APRSParser, Rule},
    Report,
};

#[test]
fn read_glidernet_sample() {
    let file = File::open("tests/glidernet-example.txt").expect("open file");
    let reader = BufReader::new(file);
    let (mut success, mut failure) = (0, 0);
    for line in reader.lines().map(|l| l.expect("should read line")) {
        if !line.starts_with("#") {
            match line.parse::<Report>() {
                Ok(_) => success += 1,
                Err(e) => {
                    failure += 1;
                    eprintln!("Failed: {}", e);
                }
            }
        }
    }

    println!("{} success and {} failures", success, failure);
    assert!(success > failure);
}

#[test]
fn parse_glidernet_sample() {
    let file = File::open("tests/glidernet-example.txt").expect("open file");
    let reader = BufReader::new(file);
    let (mut success, mut failure) = (0, 0);
    for line in reader.lines().map(|l| l.expect("should read line")) {
        if !line.starts_with("#") {
            match <APRSParser as pest::Parser<Rule>>::parse(Rule::aprs_report, &line) {
                Ok(_) => success += 1,
                Err(e) => {
                    failure += 1;
                    eprintln!("Failed: {}", e);
                }
            }
        }
    }

    println!("{} success and {} failures", success, failure);
    assert!(success > failure);
}
