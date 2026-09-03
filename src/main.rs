mod commands;
mod config;
mod eligibility;
mod gh;
mod graph;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};

const TOP_HELP: &str = "\
TYPICAL SESSION:
  glb next --diverse --reason       See what to work on (3 picks across tracks)
  glb update 44 --claim             Claim an issue before starting
  ...do the work...
  glb done 44 -c \"summary\"          Close + see what unblocked + suggest next

PLANNING:
  glb path --explain                Critical path with what each step unblocks
  glb stuck                         Top blockers + per-epic stuck counts
  glb tree 38                       Recursive sub-issue tree of an epic
  glb deps 44                       What does #44 unblock / wait on?
  glb closed --since 7d             What shipped recently?

LEARN MORE: glb <command> --help";

#[derive(Parser)]
#[command(
    name = "glb",
    about = "GitHub Issues + Projects workflow CLI",
    version,
    after_help = TOP_HELP
)]
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
        /// Answer every prompt with its default, for unattended runs
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// List open issues with optional filters
    #[command(after_help = "EXAMPLES:
  glb list                          All open issues
  glb list -p P0                    Only P0 issues
  glb list -s \"In Progress\"         Only currently-claimed issues
  glb list -a alice                 Only alice's issues")]
    List {
        #[arg(long, short = 's')]
        status: Option<String>,
        #[arg(long, short = 'p')]
        priority: Option<String>,
        #[arg(long, short = 'a')]
        assignee: Option<String>,
    },

    /// Show an issue with status, priority, and dependencies
    #[command(after_help = "EXAMPLES:
  glb show 44                       Full details, deps, sub-issues, body")]
    Show { number: u64 },

    /// Create a new issue
    #[command(after_help = "EXAMPLES:
  glb create -t \"Add login form\" -p P1 -s Todo --points 3
  glb create -t \"Bug: tests crash\" -p P0 -l bug --points 1
  glb create -t \"Auth epic\"         Then use `glb sub add` to attach children
  glb create -t \"Jars\" -m 0.1.0     File it against a release milestone

NOTES:
  - Em and en dashes (— –) are rejected. Use a hyphen '-' or colon ':'.
  - Use Fibonacci for points: 1, 2, 3, 5, 8, 13.")]
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
        /// Milestone title, e.g. a release. Must already exist on the repo
        #[arg(long, short = 'm')]
        milestone: Option<String>,
        /// Add the `autopilot` label, marking the issue claimable by an autonomous agent
        #[arg(long)]
        autopilot: bool,
    },

    /// Update status, priority, or assignee on an issue
    #[command(after_help = "EXAMPLES:
  glb update 44 --claim             Set status to In Progress (shorthand)
  glb update 44 -p P1               Set priority
  glb update 44 -s Todo             Set status
  glb update 44 --points 5          Set points (Fibonacci: 1,2,3,5,8,13)
  glb update 44 -a alice            Reassign
  glb update 44 -m 0.1.0            Put it in a release milestone
  glb update 44 -m \"\"              Take it out of its milestone")]
    Update {
        number: u64,
        #[arg(long, short = 't')]
        title: Option<String>,
        #[arg(long, short = 'b')]
        body: Option<String>,
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
        /// Milestone title, e.g. a release. Must already exist on the repo
        #[arg(long, short = 'm')]
        milestone: Option<String>,
    },

    /// Close an issue
    #[command(after_help = "EXAMPLES:
  glb close 44                      Just close
  glb close 44 -c \"Implemented X\"   Close with a closing comment

SEE ALSO: glb done — same thing plus shows what newly unblocked.")]
    Close {
        number: u64,
        #[arg(long, short = 'c')]
        comment: Option<String>,
    },

    /// Reopen a closed issue
    Reopen { number: u64 },

    /// Search issues by text
    #[command(after_help = "EXAMPLES:
  glb search \"login\"               Match by text in title/body")]
    Search { query: String },

    /// Manage issue dependencies
    #[command(after_help = "EXAMPLES:
  glb dep add 12 11                 #12 is now blocked by #11
  glb dep list 12                   Show what #12 is waiting on
  glb dep remove 12 11              Drop the dependency

SEE ALSO: glb deps — visualize the full transitive chain.")]
    Dep {
        #[command(subcommand)]
        action: DepAction,
    },

    /// Manage sub-issues (epics)
    #[command(after_help = "EXAMPLES:
  glb sub add 10 11                 #11 becomes a sub-issue of epic #10
  glb sub list 10                   Show #10's sub-issues with progress
  glb sub remove 10 11              Detach #11 from #10

SEE ALSO: glb tree — recursive sub-tree view with status icons.")]
    Sub {
        #[command(subcommand)]
        action: SubAction,
    },

    /// Show unblocked open issues (ready to work)
    #[command(after_help = "EXAMPLES:
  glb ready                         Flat list of unblocked, unclaimed issues
  glb ready --autopilot             Only issues an autonomous agent may claim
  glb ready --autopilot --explain   ...and why each other issue was skipped

TIP: For scored recommendations + parallel-agent splits, use `glb next`.")]
    Ready {
        /// Restrict to issues labelled `autopilot`
        #[arg(long)]
        autopilot: bool,
        /// Print one line per skipped issue explaining why it was not selected
        #[arg(long)]
        explain: bool,
    },

    /// Show all blocked open issues
    #[command(after_help = "EXAMPLES:
  glb blocked                       List blocked issues with their blockers

SEE ALSO: glb stuck — ranked bottlenecks (which blockers are most impactful).")]
    Blocked,

    /// Show open/closed/blocked/ready counts
    Stats,

    /// Show the critical path and high-leverage issues
    #[command(after_help = "EXAMPLES:
  glb path                          Critical path by points + top leverage
  glb path --explain                Annotate each path node with what it unblocks
  glb path --epic 38                Critical path within an epic
  glb path --by-count               Weight by issue count instead of points
  glb path --top 10                 Show top 10 high-leverage issues

WHAT IT IS:
  Critical path = the longest weighted dependency chain through open work.
  High-leverage = issues that, when finished, unblock the most downstream work.

SEE ALSO: glb next — pick a parallel batch from this analysis.")]
    Path {
        /// Use issue count instead of points for path weight
        #[arg(long)]
        by_count: bool,
        /// Number of high-leverage issues to show
        #[arg(long, default_value = "5")]
        top: usize,
        /// Scope analysis to a specific epic (and its sub-issues, recursively)
        #[arg(long, short = 'e')]
        epic: Option<u64>,
        /// Annotate each path node with what it unblocks
        #[arg(long, short = 'x')]
        explain: bool,
    },

    /// List recently closed issues
    #[command(after_help = "EXAMPLES:
  glb closed --since 1d             Yesterday's closes
  glb closed --since 7d             Last week
  glb closed --since 2w             Last two weeks
  glb closed --since 1m             Last month
  glb closed --since 2025-04-01     Since an absolute date
  glb closed --since 7d --in-epic 38   Last week, scoped to epic #38
  glb closed --limit 50             More results (default 30)

SEE ALSO: glb done — close + show what newly unblocked.")]
    Closed {
        /// Filter to issues closed since this date (YYYY-MM-DD or relative: 1d, 7d, 2w, 1m)
        #[arg(long, short = 's')]
        since: Option<String>,
        /// Filter to sub-issues of an epic (recursively)
        #[arg(long)]
        in_epic: Option<u64>,
        /// Max results
        #[arg(long, default_value = "30")]
        limit: usize,
    },

    /// Close an issue + show what newly unblocked + suggest next picks
    #[command(after_help = "EXAMPLES:
  glb done 44                       Close + analysis
  glb done 44 -c \"Implemented X\"    Close with a comment

OUTPUT INCLUDES:
  - Newly unblocked issues (only-blocker-was-this)
  - Partially-waiting issues (still have other blockers)
  - Parent epic progress
  - Suggested next: glb next --diverse --reason

SEE ALSO: glb close — same close, no analysis.")]
    Done {
        number: u64,
        #[arg(long, short = 'c')]
        comment: Option<String>,
    },

    /// Show an epic's full sub-issue tree with status icons
    #[command(after_help = "EXAMPLES:
  glb tree 38                       Full sub-issue tree of epic #38
  glb tree 10                       Works on any issue with sub-issues

ICONS:
  ✓  closed sub-issue
  ○  open sub-issue (READY tag if unblocked)

SEE ALSO: glb sub list — flat one-level view.")]
    Tree { number: u64 },

    /// Show transitive upstream/downstream dependencies as a tree
    #[command(after_help = "EXAMPLES:
  glb deps 44                       Both directions
  glb deps 44 --downstream          What does #44 unblock (transitively)?
  glb deps 44 --upstream            What is #44 waiting on (transitively)?

SEE ALSO: glb dep list — flat direct-only list.
          glb path --explain — see this in critical-path context.")]
    Deps {
        number: u64,
        /// Show only upstream (issues this is blocked by, transitively)
        #[arg(long, short = 'u')]
        upstream: bool,
        /// Show only downstream (issues this unblocks, transitively)
        #[arg(long, short = 'd')]
        downstream: bool,
    },

    /// Show top blockers and per-epic stuck counts
    #[command(after_help = "EXAMPLES:
  glb stuck                         Top 10 blockers + stuck counts per epic
  glb stuck --top 5                 Tighter view

WHAT IT TELLS YOU:
  - Which issues are bottlenecks (rank by direct issues blocked)
  - Which epics have the most stuck sub-issues
  - 'Unsticking the top 3 would unblock N issues' summary

SEE ALSO: glb blocked — flat list of all blocked issues.")]
    Stuck {
        /// How many top blockers / epics to show
        #[arg(long, default_value = "10")]
        top: usize,
    },

    /// Recommend the next batch of issues for parallel agents
    #[command(after_help = "EXAMPLES:
  glb next                          3 picks, default scoring
  glb next --agents 4 --diverse     4 picks, one per parent epic
  glb next --reason                 Show what each pick unblocks + why chosen
  glb next --track Social           Scope to an epic by name (substring match)
  glb next --epic 38                Scope to epic #38 and its sub-issues
  glb next --exclude 44 --exclude 67   Skip these candidates
  glb next --diverse --reason       The recommended planner-mode invocation

SCORING:
  priority weight + (transitive unblocks * 5) + parent-epic credit
  + critical-path bonus + small-issue bonus

SEE ALSO: glb done — closes an issue and points you back here.
          glb path — see the critical path this draws from.")]
    Next {
        /// Number of parallel agents to assign work to
        #[arg(long, default_value = "3")]
        agents: usize,
        /// Scope picks to a specific epic (and its sub-issues, recursively)
        #[arg(long, short = 'e')]
        epic: Option<u64>,
        /// Scope picks to an epic resolved by name (substring match)
        #[arg(long, short = 't')]
        track: Option<String>,
        /// Spread picks across different parent epics
        #[arg(long)]
        diverse: bool,
        /// Show what each pick unblocks (by name) and why it was chosen
        #[arg(long, short = 'r')]
        reason: bool,
        /// Skip these issue numbers from candidates
        #[arg(long)]
        exclude: Vec<u64>,
    },
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
            yes,
        } => {
            commands::init::run(owner, repo, project, yes)?;
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
            mut label,
            assignee,
            priority,
            status,
            points,
            milestone,
            autopilot,
        } => {
            if autopilot && !label.iter().any(|l| l == eligibility::AUTOPILOT_LABEL) {
                label.push(eligibility::AUTOPILOT_LABEL.to_string());
            }
            commands::create::run(
                title, body, label, assignee, priority, status, points, milestone,
            )?;
        }
        Command::Update {
            number,
            title,
            body,
            status,
            priority,
            assignee,
            claim,
            points,
            milestone,
        } => {
            commands::update::run(
                number, title, body, status, priority, assignee, claim, points, milestone,
            )?;
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
        Command::Ready { autopilot, explain } => {
            commands::ready::run(autopilot, explain)?;
        }
        Command::Blocked => {
            commands::blocked::run()?;
        }
        Command::Stats => {
            commands::stats::run()?;
        }
        Command::Path {
            by_count,
            top,
            epic,
            explain,
        } => {
            commands::path::run(by_count, top, epic, explain)?;
        }
        Command::Closed {
            since,
            in_epic,
            limit,
        } => {
            commands::closed::run(since, in_epic, limit)?;
        }
        Command::Done { number, comment } => {
            commands::done::run(number, comment)?;
        }
        Command::Tree { number } => {
            commands::tree::run(number)?;
        }
        Command::Deps {
            number,
            upstream,
            downstream,
        } => {
            commands::deps::run(number, upstream, downstream)?;
        }
        Command::Stuck { top } => {
            commands::stuck::run(top)?;
        }
        Command::Next {
            agents,
            epic,
            track,
            diverse,
            reason,
            exclude,
        } => {
            commands::next::run(commands::next::NextOpts {
                agents,
                epic,
                track,
                diverse,
                reason,
                exclude,
            })?;
        }
    }

    Ok(())
}
