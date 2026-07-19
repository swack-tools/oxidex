use clap::Args;

#[derive(Args)]
pub struct RunArgs {
    #[arg(long)]
    pub only_group: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub skip_write: bool,
    #[arg(long)]
    pub reread: bool,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
}

pub fn run(_args: RunArgs) -> anyhow::Result<()> {
    Ok(())
}
