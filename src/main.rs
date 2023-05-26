use anyhow::Result;
use clap::Parser;
use ddp2ass::Args;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    pretty_env_logger::try_init_timed()?;

    let args = load_args()?;
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

fn load_args() -> Result<Args> {
    let mut args = Args::parse();

    args.check()?;

    Ok(args)
}
