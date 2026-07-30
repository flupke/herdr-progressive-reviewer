//! Progressive reviewer pane process.

use pr_app::runtime::Runtime;

fn main() -> eyre::Result<()> {
    Runtime::from_env()?.run()
}
