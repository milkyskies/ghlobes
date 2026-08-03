/// Opt-in, never opt-out. An issue must carry this label before an autonomous agent may pick it up, so forgetting to label something means an agent leaves it alone rather than running unattended on work nobody vetted.
pub const AUTOPILOT_LABEL: &str = "autopilot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ineligible {
    MissingLabel,
}

impl Ineligible {
    pub fn reason(&self) -> String {
        match self {
            Ineligible::MissingLabel => format!("no `{AUTOPILOT_LABEL}` label"),
        }
    }
}

/// Decide whether an autonomous agent may claim this issue.
///
/// The label is the whole gate, because consent and specification are different questions and only the first is this tool's to answer. Whether an issue says enough to finish is a judgement the agent makes while reading it, and it already has somewhere to put that answer: the decision-comment protocol parks an underspecified issue on `Needs Decision` with the question, its options, and the issue body the agent needed. A grep for two heading names cannot tell a thin spec from a thorough one filed under a different heading, and in practice it rejected the latter.
///
/// Pure and side-effect free: an ineligible issue is skipped, never reassigned or commented on, because it was not claimed in the first place and its status is not the dispatcher's to change.
pub fn evaluate(labels: &[String]) -> Result<(), Ineligible> {
    if !labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case(AUTOPILOT_LABEL))
    {
        return Err(Ineligible::MissingLabel);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unlabelled_issue_is_never_eligible_however_well_written() {
        assert_eq!(evaluate(&labels(&["bug"])), Err(Ineligible::MissingLabel));
    }

    #[test]
    fn issue_with_no_labels_at_all_is_not_eligible() {
        assert_eq!(evaluate(&labels(&[])), Err(Ineligible::MissingLabel));
    }

    #[test]
    fn labelled_issue_is_eligible() {
        assert_eq!(evaluate(&labels(&["autopilot"])), Ok(()));
    }

    #[test]
    fn label_matches_case_insensitively() {
        assert_eq!(evaluate(&labels(&["Autopilot"])), Ok(()));
    }

    #[test]
    fn label_is_found_among_the_others() {
        assert_eq!(evaluate(&labels(&["bug", "P1", "autopilot"])), Ok(()));
    }

    #[test]
    fn reason_names_the_label() {
        assert_eq!(Ineligible::MissingLabel.reason(), "no `autopilot` label");
    }
}
