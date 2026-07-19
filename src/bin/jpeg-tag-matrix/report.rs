use clap::Args;

#[derive(Args)]
pub struct ReportArgs {
    #[arg(long)]
    pub update_baseline: bool,
    #[arg(long)]
    pub check_baseline: bool,
}

pub fn run(_args: ReportArgs) -> anyhow::Result<()> {
    Ok(())
}
