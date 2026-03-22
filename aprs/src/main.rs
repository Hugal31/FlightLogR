use std::fs::File;
use std::io::{BufRead as _, BufReader};

use aprs::Report;

fn main() {
    let args = std::env::args();
    if args.len() < 2 {
        return;
    }

    let file = File::open(args.skip(1).next().unwrap()).expect("Should open the file");
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.unwrap();
        match line.parse::<Report>() {
            Ok(r) => println!("{:?}", r),
            //Err(e) => eprintln!("Could not parse {line}: {e:?}"),
            _ => (),
        }
    }
}
