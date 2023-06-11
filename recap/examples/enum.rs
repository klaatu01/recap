use recap::Recap;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Recap)]
#[recap(regex = r"^(?P<room_name>\d+)\s(?P<room_capacity>\d+)")]
struct TestCommand {
    room_name: String,
    room_capacity: usize,
}

#[derive(Deserialize, Serialize, Debug, Recap)]
enum Command {
    #[recap(regex = r"^/join\s(?P<user_id>.+)")]
    Join(String),
    #[recap(regex = r"^/send\s(?P<room_id>\d+)\s(?P<message>.*)")]
    SendMessage { message: String, room_id: usize },
    #[recap(regex = r"^/test\s(?P<test>.+)")]
    CreateRoom(TestCommand),
    #[recap(regex = r"^/ping")]
    Ping,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let send_message: Command = "/send 5458 Hello, World!".parse()?;
    if let Command::SendMessage { message, room_id } = send_message {
        println!("Room({room_id}):{message}")
    };
    Ok(())
}
