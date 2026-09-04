//! One record per line, as JSON. The shape every store here is written in.
//!
//! **A separator that can appear inside a value is a format one careless field
//! breaks**, and this one did. The stores were tab separated with the name
//! last, so a player joining as `"x\n1\t<somebody else's id>\t9999"` wrote a
//! *second* row, and that row came back on the next start as that person with
//! a rating they had not earned. [`crate::net::player_name`] forbids the two
//! characters, which works only for as long as every future field remembers to.
//! JSON escapes them instead, so there is nothing left to remember.
//!
//! **One object a line, rather than one array for the file.** A whole-file
//! array parses all-or-nothing: a truncated write costs every row rather than
//! the last one, and a row this build cannot read takes the file with it. A
//! line still greps, still diffs a row at a time, and is still something to
//! open with `cat` when a server is behaving oddly — which is what the format
//! this replaces was chosen for and what it keeps.
//!
//! **A version per row, not per file**, because rows are what is read
//! forward: a store that gains a field gains it a row at a time, and a line
//! this build does not understand is skipped rather than fatal. Losing one
//! person's row is a nuisance; refusing to start is not better.

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Write rows out, one JSON object a line.
///
/// The caller sorts. Two saves of one table have to be the same bytes or a
/// diff between them says what moved rather than what changed, and a `HashMap`
/// has no order to offer.
pub fn write<T: Serialize>(rows: impl IntoIterator<Item = T>) -> String {
    let mut out = String::new();
    for row in rows {
        match serde_json::to_string(&row) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            // Not reachable for the types here, which are plain data. Dropping
            // the row rather than the file is the same trade the reader makes.
            Err(why) => log::error!("a row would not be written: {why}"),
        }
    }
    out
}

/// Read them back, skipping any line this build cannot make sense of.
///
/// `what` names the store in the warning, because "skipped a line" without it
/// is a log entry nobody can act on.
pub fn read<T: DeserializeOwned>(text: &str, what: &str) -> Vec<T> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .filter_map(|(n, line)| match serde_json::from_str(line) {
            Ok(row) => Some(row),
            Err(why) => {
                log::warn!("skipped line {} of {what}: {why}", n + 1);
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Row {
        v: u8,
        name: String,
        n: u32,
    }

    fn row(name: &str, n: u32) -> Row {
        Row { v: 1, name: name.into(), n }
    }

    /// Written down and read back, and the same bytes each time, so one save is
    /// comparable with the one before it.
    #[test]
    fn rows_survive_being_written_down() {
        let rows = vec![row("alice", 1), row("bob", 2)];
        let text = write(&rows);
        assert_eq!(text.lines().count(), 2, "a row is a line");
        assert_eq!(text, write(&rows), "two writes of one table differ");
        assert_eq!(read::<Row>(&text, "test"), rows);
    }

    /// **The whole reason for the format.** A value carrying the character that
    /// separates values is what broke the store this replaces; here it is
    /// escaped and comes back as itself.
    #[test]
    fn a_value_cannot_write_its_own_row() {
        let nasty = "x\n{\"v\":1,\"name\":\"victim\",\"n\":9999}\t\"";
        let text = write([row(nasty, 1)]);
        assert_eq!(text.lines().count(), 1, "a value wrote its own row:\n{text}");

        let back = read::<Row>(&text, "test");
        assert_eq!(back.len(), 1, "a value forged a row");
        assert_eq!(back[0].name, nasty, "and did not survive being escaped");
    }

    /// A line this build cannot read is skipped rather than fatal, which is why
    /// this is a line at a time and not one array for the file.
    #[test]
    fn a_bad_line_does_not_take_the_good_ones() {
        let text = concat!(
            "{\"v\":1,\"name\":\"alice\",\"n\":1}\n",
            "{\"v\":1,\"name\":\"from the future\"}\n",
            "not json at all\n",
            "\n",
            "{\"v\":1,\"name\":\"bob\",\"n\":2}\n",
        );
        let back = read::<Row>(text, "test");
        assert_eq!(back, vec![row("alice", 1), row("bob", 2)], "a bad line took a good one");
    }
}
