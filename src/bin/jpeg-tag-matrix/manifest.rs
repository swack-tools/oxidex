use clap::Args;

#[derive(Args)]
pub struct ManifestArgs {
    #[arg(long)]
    pub flag_noops: bool,
}

pub fn run(_args: ManifestArgs) -> anyhow::Result<()> {
    Ok(())
}
