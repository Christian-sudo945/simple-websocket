use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::thread;
use std::process;

fn main() {
    println!("Connecting to chat server...");
    let stream = match TcpStream::connect("127.0.0.1:8080") {
        Ok(stream) => stream,
        Err(_) => {
            println!("Could not connect to server");
            process::exit(1);
        }
    };
    println!("Connected to server!");
    println!("Type your messages (type 'quit' to exit):");

    let mut write_stream = stream.try_clone().unwrap();
    let read_stream = stream;

    let receive_handle = thread::spawn(move || {
        let mut reader = BufReader::new(read_stream);
        let mut line = String::new();
        
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    println!("\nServer closed connection");
                    process::exit(0);
                }
                Ok(_) => {
                    print!("{}", line);
                }
                Err(_) => {
                    println!("\nLost connection to server");
                    process::exit(1);
                }
            }
        }
    });

    let mut input = String::new();
    let stdin = io::stdin();
    let mut handle = stdin.lock();

    loop {
        input.clear();
        if handle.read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();
        if input.eq_ignore_ascii_case("quit") {
            break;
        }

        if !input.is_empty() {
            if write_stream.write_all(input.as_bytes()).is_err() || 
               write_stream.write_all(b"\n").is_err() {
                println!("Failed to send message");
                break;
            }
        }
    }

    drop(write_stream);
    receive_handle.join().unwrap();
}