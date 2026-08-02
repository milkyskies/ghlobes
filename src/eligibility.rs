/// Opt-in, never opt-out. An issue must carry this label before an autonomous agent may pick it up, so forgetting to label something means an agent leaves it alone rather than running unattended on work nobody vetted.
pub const AUTOPILOT_LABEL: &str = "autopilot";

/// Sections an agent needs in order to work unattended and to know when it is finished. Absence of either is a reliable proxy for "a human should drive this".
const REQUIRED_SECTIONS: [&str; 2] = ["Acceptance criteria", "Tests"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ineligible {
    MissingLabel,
    MissingSection(String),
}

impl Ineligible {
    pub fn reason(&self) -> String {
        match self {
            Ineligible::MissingLabel => format!("no `{AUTOPILOT_LABEL}` label"),
            Ineligible::MissingSection(name) => format!("labelled, missing `## {name}`"),
        }
    }
}

/// Decide whether an autonomous agent may claim this issue.
///
/// Pure and side-effect free: an ineligible issue is skipped, never reassigned or commented on, because it was not claimed in the first place and its status is not the dispatcher's to change.
pub fn evaluate(labels: &[String], body: &str) -> Result<(), Ineligible> {
    if !labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case(AUTOPILOT_LABEL))
    {
        return Err(Ineligible::MissingLabel);
    }

    for section in REQUIRED_SECTIONS {
        if !has_content_under_heading(body, section) {
            return Err(Ineligible::MissingSection(section.to_string()));
        }
    }

    Ok(())
}

/// A heading with nothing under it is treated as absent, because an empty `## Tests` satisfies a grep while telling an agent nothing.
///
/// The section runs until the next heading at the same or a shallower level, so `### Domain` nested under `## Tests` is part of that section rather than the end of it.
fn has_content_under_heading(body: &str, heading: &str) -> bool {
    let mut lines = body.lines();

    let level = loop {
        match lines.next() {
            Some(line) => match heading_level_for(line, heading) {
                Some(level) => break level,
                None => continue,
            },
            None => return false,
        }
    };

    lines
        .take_while(|line| heading_level(line).is_none_or(|found| found > level))
        .any(|line| !line.trim().is_empty())
}

fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || !trimmed[hashes..].starts_with(' ') {
        return None;
    }
    Some(hashes)
}

fn heading_level_for(line: &str, heading: &str) -> Option<usize> {
    let level = heading_level(line)?;
    let text = line.trim_start().trim_start_matches('#').trim();
    text.eq_ignore_ascii_case(heading).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPLETE: &str = "\
## Problem
Something is broken.

## Acceptance criteria
- It stops being broken

## Tests
- Asserts it is not broken
";

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unlabelled_issue_is_never_eligible_however_well_formed() {
        assert_eq!(
            evaluate(&labels(&["bug"]), COMPLETE),
            Err(Ineligible::MissingLabel)
        );
    }

    #[test]
    fn labelled_and_complete_issue_is_eligible() {
        assert_eq!(evaluate(&labels(&["autopilot"]), COMPLETE), Ok(()));
    }

    #[test]
    fn label_matches_case_insensitively() {
        assert_eq!(evaluate(&labels(&["Autopilot"]), COMPLETE), Ok(()));
    }

    #[test]
    fn issue_without_tests_section_is_not_eligible() {
        let body = "## Acceptance criteria\n- It works\n";
        assert_eq!(
            evaluate(&labels(&["autopilot"]), body),
            Err(Ineligible::MissingSection("Tests".to_string()))
        );
    }

    #[test]
    fn issue_without_acceptance_criteria_is_not_eligible() {
        let body = "## Tests\n- Asserts it works\n";
        assert_eq!(
            evaluate(&labels(&["autopilot"]), body),
            Err(Ineligible::MissingSection(
                "Acceptance criteria".to_string()
            ))
        );
    }

    #[test]
    fn empty_section_counts_as_missing() {
        let body = "## Acceptance criteria\n- It works\n\n## Tests\n\n## Notes\nsomething\n";
        assert_eq!(
            evaluate(&labels(&["autopilot"]), body),
            Err(Ineligible::MissingSection("Tests".to_string()))
        );
    }

    #[test]
    fn trailing_section_with_no_content_counts_as_missing() {
        let body = "## Acceptance criteria\n- It works\n\n## Tests\n";
        assert_eq!(
            evaluate(&labels(&["autopilot"]), body),
            Err(Ineligible::MissingSection("Tests".to_string()))
        );
    }

    #[test]
    fn heading_matches_case_insensitively() {
        let body = "## ACCEPTANCE CRITERIA\n- It works\n\n## tests\n- Asserts it\n";
        assert_eq!(evaluate(&labels(&["autopilot"]), body), Ok(()));
    }

    #[test]
    fn deeper_heading_levels_are_accepted() {
        let body = "### Acceptance criteria\n- It works\n\n### Tests\n- Asserts it\n";
        assert_eq!(evaluate(&labels(&["autopilot"]), body), Ok(()));
    }

    #[test]
    fn subsections_do_not_terminate_a_section_prematurely() {
        let body = "## Acceptance criteria\n- It works\n\n## Tests\n### Domain\n- Asserts it\n";
        assert_eq!(evaluate(&labels(&["autopilot"]), body), Ok(()));
    }

    #[test]
    fn empty_body_is_not_eligible() {
        assert_eq!(
            evaluate(&labels(&["autopilot"]), ""),
            Err(Ineligible::MissingSection(
                "Acceptance criteria".to_string()
            ))
        );
    }

    #[test]
    fn reasons_name_what_is_missing() {
        assert_eq!(Ineligible::MissingLabel.reason(), "no `autopilot` label");
        assert_eq!(
            Ineligible::MissingSection("Tests".to_string()).reason(),
            "labelled, missing `## Tests`"
        );
    }
}
