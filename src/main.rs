use anyhow::Result;
use clap::Parser;
use ddp2ass::{Args, Cli, Commands};
use simplelog::*;

#[tokio::main]
async fn main() -> Result<()> {
    let config = ConfigBuilder::new()
        .set_time_offset_to_local()
        .unwrap()
        .build();
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Info,
        config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )])?;

    let cli = Cli::parse();
    return match cli.command {
        Some(Commands::Download(args)) => download(args).await,
        Some(Commands::MatchParams(args)) => args.process(),
        Some(Commands::MatchResult(args)) => args.process().await,
        None => {
            let args = Args::parse();
            download(args).await
        }
    };
}

async fn download(mut args: Args) -> Result<()> {
    args.check()?;

    let pause = args.pause;

    let ret = args.process().await;
    if pause {
        if let Err(e) = ret.as_ref() {
            println!();
            eprintln!("发生错误：{:?}", e);
        }

        println!("按任意键继续");
        std::io::stdin().read_line(&mut String::new())?;
    }

    ret
}
