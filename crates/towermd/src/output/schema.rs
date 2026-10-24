//! Output schemas for every visible leaf command, embedded at build time.
use serde_json::Value;

pub const ENVELOPE: &str = include_str!("../../schema/envelope.json");
/// Bare `towermd`, which is not a leaf of the command tree.
pub const ROOT: &str = include_str!("../../schema/towermd.json");

/// (dotted command name, schema of the envelope's `data` on success)
pub const COMMANDS: &[(&str, &str)] = &[
    ("up", include_str!("../../schema/up.json")),
    ("show", include_str!("../../schema/show.json")),
    ("needs", include_str!("../../schema/needs.json")),
    ("since", include_str!("../../schema/since.json")),
    ("overview", include_str!("../../schema/overview.json")),
    ("ls", include_str!("../../schema/ls.json")),
    ("history", include_str!("../../schema/history.json")),
    ("watch", include_str!("../../schema/watch.json")),
    ("open", include_str!("../../schema/open.json")),
    ("decision.show", include_str!("../../schema/decision.show.json")),
    ("decision.ls", include_str!("../../schema/decision.ls.json")),
    ("rule.ls", include_str!("../../schema/rule.ls.json")),
    ("rebuild-index", include_str!("../../schema/rebuild-index.json")),
    ("doctor", include_str!("../../schema/doctor.json")),
    ("whoami", include_str!("../../schema/whoami.json")),
    ("completion", include_str!("../../schema/completion.json")),
    ("task.add", include_str!("../../schema/task.add.json")),
    ("task.edit", include_str!("../../schema/task.edit.json")),
    ("task.ls", include_str!("../../schema/task.ls.json")),
    ("task.claim", include_str!("../../schema/task.claim.json")),
    ("task.show", include_str!("../../schema/task.show.json")),
    ("task.release", include_str!("../../schema/task.release.json")),
    ("propose", include_str!("../../schema/propose.json")),
    ("pause", include_str!("../../schema/pause.json")),
    ("resume", include_str!("../../schema/resume.json")),
    ("status", include_str!("../../schema/status.json")),
    ("ask", include_str!("../../schema/ask.json")),
    ("wait", include_str!("../../schema/wait.json")),
    ("answer", include_str!("../../schema/answer.json")),
    ("handled", include_str!("../../schema/handled.json")),
    ("reply", include_str!("../../schema/reply.json")),
    ("report", include_str!("../../schema/report.json")),
    ("signal", include_str!("../../schema/signal.json")),
    ("idea.add", include_str!("../../schema/idea.add.json")),
    ("ext.ls", include_str!("../../schema/ext.ls.json")),
    ("ext.show", include_str!("../../schema/ext.show.json")),
    ("template.ls", include_str!("../../schema/template.ls.json")),
    ("frame.show", include_str!("../../schema/frame.show.json")),
    ("frame.add", include_str!("../../schema/frame.add.json")),
    ("frame.update", include_str!("../../schema/frame.update.json")),
    ("frame.move", include_str!("../../schema/frame.move.json")),
    ("frame.link", include_str!("../../schema/frame.link.json")),
    ("frame.archive", include_str!("../../schema/frame.archive.json")),
    ("frame.propose", include_str!("../../schema/frame.propose.json")),
    ("frame.edit", include_str!("../../schema/frame.edit.json")),
    ("snapshot", include_str!("../../schema/snapshot.json")),
    ("hub.rebuild", include_str!("../../schema/hub.rebuild.json")),
    ("daemon.ensure", include_str!("../../schema/daemon.ensure.json")),
    ("daemon.status", include_str!("../../schema/daemon.status.json")),
    ("daemon.stop", include_str!("../../schema/daemon.stop.json")),
    ("init", include_str!("../../schema/init.json")),
    ("dev.check", include_str!("../../schema/dev.check.json")),
    ("dev.finish", include_str!("../../schema/dev.finish.json")),
    ("dev.clarify", include_str!("../../schema/dev.clarify.json")),
    ("dev.trailers", include_str!("../../schema/dev.trailers.json")),
    ("dev.scan.start", include_str!("../../schema/dev.scan.start.json")),
    ("dev.scan.wait", include_str!("../../schema/dev.scan.wait.json")),
    ("dev.scan.show", include_str!("../../schema/dev.scan.show.json")),
    ("dev.scan.cancel", include_str!("../../schema/dev.scan.cancel.json")),
    ("dev.draft.template", include_str!("../../schema/dev.draft.template.json")),
    ("dev.draft.submit", include_str!("../../schema/dev.draft.submit.json")),
    ("dev.draft.show", include_str!("../../schema/dev.draft.show.json")),
    ("dev.draft.ls", include_str!("../../schema/dev.draft.ls.json")),
    ("dev.draft.compare", include_str!("../../schema/dev.draft.compare.json")),
    ("schema", include_str!("../../schema/schema.json")),
];

pub fn find(name: &str) -> Option<Value> {
    COMMANDS.iter().find(|(command, _)| *command == name).map(|(_, text)| serde_json::from_str(text).expect("embedded schema is JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Cli;
    use clap::CommandFactory;
    use std::collections::BTreeSet;

    fn leaves(command: &clap::Command, prefix: &str, out: &mut BTreeSet<String>) {
        for sub in command.get_subcommands().filter(|sub| !sub.is_hide_set() && sub.get_name() != "help") {
            let name = if prefix.is_empty() { sub.get_name().to_owned() } else { format!("{prefix}.{}", sub.get_name()) };
            if sub.has_subcommands() { leaves(sub, &name, out) } else { out.insert(name); }
        }
    }

    #[test]
    fn every_visible_leaf_command_has_exactly_one_schema() {
        let mut expected = BTreeSet::new();
        leaves(&Cli::command(), "", &mut expected);
        let actual: BTreeSet<String> = COMMANDS.iter().map(|(name, _)| (*name).to_owned()).collect();
        assert_eq!(actual, expected);
        assert_eq!(expected.len(), 65);
        for (name, text) in COMMANDS.iter().chain([&("envelope", ENVELOPE), &("towermd", ROOT)]) {
            let schema: Value = serde_json::from_str(text).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert!(schema["type"] == "object" || schema["type"] == "array", "{name}");
        }
        let files = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/schema")).unwrap().count();
        assert_eq!(files, 67, "one file per command plus envelope.json and towermd.json");
    }
}
