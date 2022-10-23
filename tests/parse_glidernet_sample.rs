use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};

use flightlogr::aprs::client::{APRSClient, Credentials};
use flightlogr::aprs::{
    client::Reports,
    report::parsing::{APRSParser, Rule},
};

#[test]
fn read_glidernet_sample() {
    let file = File::open("tests/glidernet-example.txt").expect("open file");
    let reports = Reports::new(file);
    let (mut success, mut failure) = (0, 0);
    for report in reports {
        match report {
            Ok(_) => success += 1,
            Err(e) => {
                failure += 1;
                eprintln!("Failed: {}", e);
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

#[test]
fn test_server_errors() {
    let (mut server, client) = socketpair::socketpair_stream().expect("should have created socket");
    let creds = Credentials {
        user: "user".to_string(),
        password: "pass".to_string(),
        app_name: "tests".to_string(),
        app_version: "0.1.0".to_string(),
    };

    let client_thread = std::thread::spawn(move || APRSClient::login(client, &creds, &[]));

    let mut buff = [0; 1024];
    let read_size = server.read(&mut buff).expect("should read");
    server.write(b"# APRS\n").unwrap();
    server.read(&mut buff).unwrap();
    assert_eq!(
        &buff[..read_size],
        "user user pass pass vers tests 0.1.0\n".as_bytes(),
        "{} != {}",
        String::from_utf8_lossy(&buff[..read_size]),
        "user user pass pass vers tests 0.1.0\n"
    );
    server.write(b"# logresp verified\n").unwrap();
    server.flush().unwrap();

    client_thread.join().unwrap().unwrap();
}
