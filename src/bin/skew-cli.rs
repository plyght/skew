use skew::ipc::IpcClient;
use tokio;

#[tokio::main]
async fn main() -> skew::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: skew-cli <command> [args...]");
        eprintln!("Commands: ping, help, list, status, toggle-layout, quit");
        std::process::exit(1);
    }
    
    let command = &args[1];
    let command_args = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        vec![]
    };
    
    let socket_path = "/tmp/skew.sock";
    
    match IpcClient::run_command(socket_path, command, command_args).await {
        Ok(()) => {},
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
    
    Ok(())
}