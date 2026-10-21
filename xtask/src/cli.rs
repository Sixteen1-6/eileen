//! The command line: `cargo xtask <command> [options]`, parsed by hand because xtask keeps its
//! dependencies to the three BUILD.md section 4b.5 allows.

use std::ffi::OsString;
use std::path::PathBuf;

/// The usage text printed by `cargo xtask help` and after a usage error.
pub(crate) const USAGE: &str = "\
usage: cargo xtask <command> [options]

commands:
  pr-meta   check a pull request's title, description and changed files (BUILD.md checks 7, 8, 19)
  help      print this text

cargo xtask pr-meta --body-file <file> --title <title> [--base <rev>] [--head <rev>] [--author <login>]
  --body-file  the pull request description, as Markdown
  --title      the pull request title
  --base       the branch the pull request merges into (default origin/main)
  --head       the pull request's last commit (default HEAD)
  --author     the pull request author's login; dependabot[bot] gets the bot rule (default: a person)
";

/// A parsed command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Print the usage text.
    Help,
    /// Run the pull request checks.
    PrMeta(PrMetaOptions),
}

/// Options of `cargo xtask pr-meta`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PrMetaOptions {
    /// The file holding the pull request description.
    pub(crate) body_file: PathBuf,
    /// The pull request title.
    pub(crate) title: String,
    /// The base revision.
    pub(crate) base: String,
    /// The head revision.
    pub(crate) head: String,
    /// The author's login, empty for a person who did not say.
    pub(crate) author: String,
}

/// Parses the arguments after the program name.
pub(crate) fn parse<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter().map(|arg| {
        arg.into_string().map_err(|raw| format!("argument `{}` is not valid UTF-8", raw.display()))
    });
    match args.next().transpose()?.as_deref() {
        None | Some("help" | "--help" | "-h") => Ok(Command::Help),
        Some("pr-meta") => parse_pr_meta(args).map(Command::PrMeta),
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

fn parse_pr_meta<I>(mut args: I) -> Result<PrMetaOptions, String>
where
    I: Iterator<Item = Result<String, String>>,
{
    let mut body_file = None;
    let mut title = None;
    let mut base = None;
    let mut head = None;
    let mut author = None;
    while let Some(arg) = args.next().transpose()? {
        let (flag, inline) = split_flag(arg);
        let slot = match flag.as_str() {
            "--body-file" => &mut body_file,
            "--title" => &mut title,
            "--base" => &mut base,
            "--head" => &mut head,
            "--author" => &mut author,
            _ => return Err(format!("pr-meta does not take `{flag}`")),
        };
        if slot.is_some() {
            return Err(format!("`{flag}` is given twice"));
        }
        let value = match inline {
            Some(value) => value,
            None => args.next().transpose()?.ok_or_else(|| format!("`{flag}` needs a value"))?,
        };
        *slot = Some(value);
    }
    Ok(PrMetaOptions {
        body_file: PathBuf::from(body_file.ok_or("pr-meta needs `--body-file <file>`")?),
        title: title.ok_or("pr-meta needs `--title <title>`")?,
        base: base.unwrap_or_else(|| "origin/main".to_owned()),
        head: head.unwrap_or_else(|| "HEAD".to_owned()),
        author: author.unwrap_or_default(),
    })
}

/// Splits `--flag=value` into its flag and value; anything else is a flag with no inline value.
fn split_flag(arg: String) -> (String, Option<String>) {
    if arg.starts_with("--")
        && let Some((flag, value)) = arg.split_once('=')
    {
        return (flag.to_owned(), Some(value.to_owned()));
    }
    (arg, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse_strs(&[]), Ok(Command::Help), "bare `cargo xtask` should print usage");
    }

    #[test]
    fn pr_meta_takes_both_flag_forms_and_defaults() {
        let parsed = parse_strs(&["pr-meta", "--body-file", "b.md", "--title=Add a check"]);
        assert_eq!(
            parsed,
            Ok(Command::PrMeta(PrMetaOptions {
                body_file: PathBuf::from("b.md"),
                title: "Add a check".to_owned(),
                base: "origin/main".to_owned(),
                head: "HEAD".to_owned(),
                author: String::new(),
            })),
            "flags with a space or an equals sign, base and head defaulted"
        );
    }

    #[test]
    fn pr_meta_rejects_missing_repeated_and_unknown_flags() {
        let missing = parse_strs(&["pr-meta", "--title", "Add a check"]);
        assert!(missing.is_err_and(|e| e.contains("--body-file")), "body file is required");
        let twice = parse_strs(&["pr-meta", "--title", "A", "--title", "B"]);
        assert!(twice.is_err_and(|e| e.contains("twice")), "a repeated flag is an error");
        let unknown = parse_strs(&["pr-meta", "--bodyfile", "b.md"]);
        assert!(unknown.is_err_and(|e| e.contains("--bodyfile")), "a misspelt flag is named");
        let dangling = parse_strs(&["pr-meta", "--body-file"]);
        assert!(dangling.is_err_and(|e| e.contains("needs a value")), "a flag needs its value");
    }

    #[test]
    fn unknown_command_is_named() {
        let parsed = parse_strs(&["lint-repo"]);
        assert!(parsed.is_err_and(|e| e.contains("lint-repo")), "the unknown command is named");
    }
}
