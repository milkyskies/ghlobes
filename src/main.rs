mod commands;
mod config;
mod gh;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "glb", about = "GitHub Issues + Projects workflow CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Detect project config and write .ghlobes.toml
    Init {
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        /// GitHub Project number
        #[arg(long, short = 'p')]
        project: Option<u64>,
        /// Only refresh the CLAUDE.md agent instructions, skip all other setup
        #[arg(long)]
        update_claude_md: bool,
    },

    /// List open issues with optional filters
    List {
        #[arg(long, short = 's')]
        status: Option<String>,
        #[arg(long, short = 'p')]
        priority: Option<String>,
        #[arg(long, short = 'a')]
        assignee: Option<String>,
    },

    /// Show an issue with status, priority, and dependencies
    Show { number: u64 },

    /// Create a new issue
    Create {
        #[arg(long, short = 't')]
        title: Option<String>,
        #[arg(long, short = 'b')]
        body: Option<String>,
        #[arg(long, short = 'l')]
        label: Vec<String>,
        #[arg(long, short = 'a')]
        assignee: Vec<String>,
        #[arg(long, short = 'p')]
        priority: Option<String>,
        #[arg(long, short = 's')]
        status: Option<String>,
        /// Effort estimate (use Fibonacci: 1, 2, 3, 5, 8, 13)
        #[arg(long)]
        points: Option<f64>,
    },

    /// Update status, priority, or assignee on an issue
    Update {
        number: u64,
        #[arg(long, short = 't')]
        title: Option<String>,
        #[arg(long, short = 's')]
        status: Option<String>,
        #[arg(long, short = 'p')]
        priority: Option<String>,
        #[arg(long, short = 'a')]
        assignee: Option<String>,
        /// Set status to in_progress (shorthand for --status in_progress)
        #[arg(long)]
        claim: bool,
        /// Effort estimate (use Fibonacci: 1, 2, 3, 5, 8, 13)
        #[arg(long)]
        points: Option<f64>,
    },

    /// Close an issue
    Close {
        number: u64,
        #[arg(long, short = 'c')]
        comment: Option<String>,
    },

    /// Reopen a closed issue
    Reopen { number: u64 },

    /// Search issues by text
    Search { query: String },

    /// Manage issue dependencies
    Dep {
        #[command(subcommand)]
        action: DepAction,
    },

    /// Manage sub-issues (epics)
    Sub {
        #[command(subcommand)]
        action: SubAction,
    },

    /// Show unblocked open issues (ready to work)
    Ready,

    /// Show all blocked open issues
    Blocked,

    /// Show open/closed/blocked/ready counts
    Stats,
}

#[derive(Subcommand)]
enum SubAction {
    /// Add an issue as a sub-issue of a parent
    Add {
        /// Parent issue number
        parent: u64,
        /// Child issue number
        child: u64,
    },
    /// Remove a sub-issue from a parent
    Remove { parent: u64, child: u64 },
    /// List sub-issues of a parent
    List { parent: u64 },
}

#[derive(Subcommand)]
enum DepAction {
    /// Mark issue as blocked by another issue
    Add {
        /// Issue that is blocked
        issue: u64,
        /// Issue doing the blocking
        blocked_by: u64,
    },
    /// Remove a blocked-by relationship
    Remove { issue: u64, blocked_by: u64 },
    /// List dependencies for an issue
    List { issue: u64 },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init {
            owner,
            repo,
            project,
            update_claude_md,
        } => {
            commands::init::run(owner, repo, project, update_claude_md)?;
        }
        Command::List {
            status,
            priority,
            assignee,
        } => {
            commands::list::run(status, priority, assignee)?;
        }
        Command::Show { number } => {
            commands::show::run(number)?;
        }
        Command::Create {
            title,
            body,
            label,
            assignee,
            priority,
            status,
            points,
        } => {
            commands::create::run(title, body, label, assignee, priority, status, points)?;
        }
        Command::Update {
            number,
            title,
            status,
            priority,
            assignee,
            claim,
            points,
        } => {
            commands::update::run(number, title, status, priority, assignee, claim, points)?;
        }
        Command::Close { number, comment } => {
            commands::close::run(number, comment)?;
        }
        Command::Reopen { number } => {
            commands::reopen::run(number)?;
        }
        Command::Search { query } => {
            commands::search::run(&query)?;
        }
        Command::Dep { action } => match action {
            DepAction::Add { issue, blocked_by } => commands::dep::add(issue, blocked_by)?,
            DepAction::Remove { issue, blocked_by } => commands::dep::remove(issue, blocked_by)?,
            DepAction::List { issue } => commands::dep::list(issue)?,
        },
        Command::Sub { action } => match action {
            SubAction::Add { parent, child } => commands::sub::add(parent, child)?,
            SubAction::Remove { parent, child } => commands::sub::remove(parent, child)?,
            SubAction::List { parent } => commands::sub::list(parent)?,
        },
        Command::Ready => {
            commands::ready::run()?;
        }
        Command::Blocked => {
            commands::blocked::run()?;
        }
        Command::Stats => {
            commands::stats::run()?;
        }
    }

    Ok(())
}
