//! A library and command-line tool for detecting broken links in Markdown files.
//!
//! By default, this tool detects broken links like "[foo](file.md)" (target file does not exist)
//! and broken header links like "[foo](file.md#header)" (target file exists but specific header does not exist)
//!
//! ## Command-line usage
//!
//! Check a single file:
//!
//! ```shell
//! broken-md-links input.md
//! ```
//!
//! Check a whole directory:
//!
//! ```shell
//! broken-md-links dir/ -r
//! ```
//!
//! ### Output
//!
//! There are several levels of verbosity:
//!
//! * `-v silent`: display nothing (exit code will be 0 if there was no broken link)
//! * `-v errors`: display errors only
//! * `-v warn`: display errors and warnings (the default)
//! * `-v info`: display the list of analyzed files as well
//! * `-v verbose`: display detailed informations
//! * `-v trace`: display debug informations
//!
//! Additionally, the `--no-error` flag converst all broken/invalid link errors to warnings.
//!
//! ## Library usage
//!
//! ```
//! use broken_md_links::check_broken_links;
//!
//! match check_broken_links(Path::new("file.md"), false, false, false, &mut HashMap::new()) {
//!   Ok(0) => println!("No broken link :D"),
//!   Ok(errors @ _) => println!("There are {} broken links :(", errors),
//!   Err(err) => println!("Something went wrong :( : {}", err)
//! }
//! ```

use colored::Colorize;
use lazy_static::lazy_static;
use log::{debug, error, info, trace, warn};
use pulldown_cmark::{BrokenLink, Event, LinkType, Options, Parser, Tag};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

lazy_static! {
    static ref EMAIL_REGEX: Regex = Regex::new("\
        (?:[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*|\"\
        (?:[\\x01-\\x08\\x0b\\x0c\\x0e-\\x1f\\x21\\x23-\\x5b\\x5d-\\x7f]|\\\\[\\x01-\\x09\\x0b\\x0c\\x0e-\\x7f])*\")@\
        (?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?|\\[\
        (?:(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9]))\\.){3}(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9])|[a-z0-9-]*[a-z0-9]:\
        (?:[\\x01-\\x08\\x0b\\x0c\\x0e-\\x1f\\x21-\\x5a\\x53-\\x7f]|\\\\[\\x01-\\x09\\x0b\\x0c\\x0e-\\x7f])+)\\])"
    ).unwrap();
}

/// Canonicalize a path and display it as a lossy string
///
/// # Examples
///
/// ```
/// let path = Path::new("../a/b/../c");
///
/// path.to_string_lossy();  // "../a/b/../c"
/// safe_canonicalize(path); // "../a/c"
/// ```
pub fn safe_canonicalize(path: &Path) -> String {
    // Components of the canonicalized path
    let mut out = vec![];

    for comp in path.components() {
        match comp {
            // Prefixes, root directories and normal components are kept "as is"
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => out.push(comp),

            // "Current dir" symbols (e.g. ".") are useless so they are not kept
            Component::CurDir => {}

            // "Parent dir" symbols (e.g. "..") will remove the previous component *ONLY* if it's a normal one
            // Else, if the path is relative the symbol will be kept to preserve the relativety of the path
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = out.last() {
                    out.pop();
                } else if path.is_relative() {
                    out.push(Component::ParentDir)
                }
            }
        }
    }

    // Create a path from the components and display it as a lossy string
    out.iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned()
}

/// Slugify a Markdown header
/// This function is used to generate slugs from all headers of a Markdown file (see the 'generate_slugs' function)
///
/// # Examples
///
/// ```
/// slugify("My super header") # "my-super-header"
/// slugify("I love headers!") # "i-love-headers"
/// ```
pub fn slugify(header: &str) -> String {
    header
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

/// Get all headers of a Markdown file as slugs
/// This function is used to check if the header specified in a link exists in the target file
/// Returns an error message if the operation failed for any reason
pub fn generate_slugs(path: &Path) -> Result<Vec<String>, String> {
    // Get the canonicalized path for display
    let canon = safe_canonicalize(path);

    debug!("Generating slugs for file: {}", canon);

    // Read the input file
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read file at '{}': {}", canon.green(), err))?;

    trace!(
        "In '{}': just read file, which is {} bytes long.",
        canon,
        content.len()
    );

    // The list of slugified headers
    let mut headers = vec![];

    // Counter of slugs for suffixes
    let mut header_counts = HashMap::<String, usize>::new();

    // When the 'pulldown_cmark' library encounters a heading, the actual title can be got between a Start() and an End() events
    // This variable contains the pending title's content
    let mut header: Option<String> = None;

    // Create a pull-down markdown parser
    let parser = Parser::new_ext(&content, Options::all());

    for (event, range) in parser.into_offset_iter() {
        macro_rules! format_msg {
            ($($param: expr),*) => {{
                // TODO: Optimize the computation of the line number
                let line = content.chars().take(range.start).filter(|c| *c == '\n').count();
                format!("In '{}', line {}: {}", canon.green(), (line + 1).to_string().bright_magenta(), format!($($param),*))
            }}
        }

        // If the last event was an heading, we are now expecting to get its title
        if let Some(ref mut header_str) = header {
            match event {
                // Event indicating the header is now complete
                Event::End(Tag::Heading(_)) => {
                    // Get its slug
                    let slug = slugify(&header_str);
                    debug!("{}", format_msg!("found header: #{}", slug));

                    // Print a warning if the title is empty
                    if header_str.trim().is_empty() {
                        // We did not get a piece of text, which means this heading does not have a title
                        warn!(
                            "{}",
                            format_msg!("heading was not directly followed by a title")
                        );
                        trace!("Faulty event: {:?}", event);
                    }

                    // Get the number of duplicates this slug has
                    let duplicates = header_counts
                        .entry(slug.clone())
                        .and_modify(|d| *d += 1)
                        .or_insert(0);

                    // Add a suffix for duplicates
                    if *duplicates > 0 {
                        headers.push(format!("{}-{}", slug, duplicates));
                    } else {
                        headers.push(slug);
                    }

                    // Header is now complete
                    header = None;
                }

                Event::Start(_)
                | Event::End(_)
                | Event::SoftBreak
                | Event::HardBreak
                | Event::Rule
                | Event::TaskListMarker(_) => {}
                Event::Text(text)
                | Event::Code(text)
                | Event::Html(text)
                | Event::FootnoteReference(text) => header_str.push_str(&text),
            }
        }
        // If we encounted the beginning of a heading...
        else if let Event::Start(Tag::Heading(_)) = event {
            // Expect to get the related title just after
            header = Some(String::new())
        }
    }

    // Everything went fine :D
    Ok(headers)
}

/// Check broken links in a Markdown file or directory
///
/// The input `path` will be checked recursively as a directory if `dir` is set to `true`, else as a single file.
///
/// By default, when a header points to a specific header (e.g. `other_file.md#some-header`), the target file will be opened and
///  the function will check if it contains the said header. As this feature may slow down the whole process, it's possible to disable it by
///  settings `ignore_header_links` to `true`.
///
/// In order to improve performances when looking at header-specific links, when a file's list of headers is made, it is stored inside a cache
/// This cache is shared recursively through the `links_cache` argument. As it uses a specific format, it's recommanded to just pass a mutable
///  reference to an empty HashMap to this function, and not build your own one which may cause detection problems.
///
/// If the `no_errors` parameter is set, all broken/invalid link errors will be displayed as simple warnings (but errors will still be counted).
///
/// The function returns an error is something goes wrong, or else the number of broken and invalid (without target) links.
///
/// # Examples
///
/// ```
/// // Single file
/// assert_eq(check_broken_links(Path::new("file.md"), false, false, &mut HashMap::new()), Ok(0), "There are broken/invalid links :(");
///
/// // Directory
/// assert_eq(check_broken_links(Path::new("dir/"), true, false, &mut HashMap::new()), Ok(0), "There are broken/invalid links :(");
pub fn check_broken_links(
    path: &Path,
    dir: bool,
    ignore_header_links: bool,
    no_errors: bool,
    mut links_cache: &mut HashMap<PathBuf, Vec<String>>,
) -> Result<u64, String> {
    /// Display a broken/invalid link error
    macro_rules! err_or_warn {
        ($($arg: expr),*) => {
            if no_errors {
                warn!($($arg),*);
            } else {
                error!($($arg),*);
            }
        }
    }

    // Get the canonicalized path for display
    let canon = safe_canonicalize(path);

    // Count errors
    let mut errors = 0;

    if dir {
        debug!("Analyzing directory: {}", canon);

        for item in path.read_dir().map_err(|err| {
            format!(
                "Failed to read input directory at '{}': {}",
                canon.green(),
                err
            )
        })? {
            let item = item.map_err(|err| {
                format!(
                    "Failed to get item from directory at '{}': {}",
                    canon.green(),
                    err
                )
            })?;
            let path = item.path();
            let file_type = item.file_type().map_err(|err| {
                format!(
                    "Failed to read file type of item at '{}': {}",
                    canon.green(),
                    err
                )
            })?;

            if file_type.is_dir() {
                // Check broken links recursively
                errors += check_broken_links(
                    &path,
                    true,
                    ignore_header_links,
                    no_errors,
                    &mut links_cache,
                )?;
            } else if file_type.is_file() {
                // Only check ".md" files
                if let Some(ext) = path.extension() {
                    if let Some(ext) = ext.to_str() {
                        if ext == "md" {
                            // Check this Markdown file
                            errors += check_broken_links(
                                &path,
                                false,
                                ignore_header_links,
                                no_errors,
                                links_cache,
                            )?;
                        }
                    }
                }
            } else {
                warn!(
                    "Item at path '{}' is neither a file nor a directory so it will be ignored",
                    canon
                );
            }
        }
    } else {
        // Treat input as a file
        info!("Analyzing: {}", canon);

        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("Failed to read file at '{}': {}", canon.green(), err))?;

        trace!(
            "In '{}': just read file, which is {} bytes long.",
            canon,
            content.len()
        );

        // Count links without a target (like `[link name]`) as an error
        let mut handle_broken_links = |link: BrokenLink| {
            err_or_warn!(
                "In '{}': Missing target for link '{}'",
                canon.green(),
                link.reference.yellow()
            );

            None
        };

        // Create a pull-down parser
        let parser = Parser::new_with_broken_link_callback(
            &content,
            Options::all(),
            Some(&mut handle_broken_links),
        );

        for (event, range) in parser.into_offset_iter() {
            macro_rules! format_msg {
                ($($param: expr),*) => {{
                    // TODO: Optimize the computation of the line number
                    let line = content.chars().take(range.start).filter(|c| *c == '\n').count();
                    format!("In '{}', line {}: {}", canon.green(), line + 1, format!($($param),*))
                }}
            }

            // Check links only
            if let Event::End(Tag::Link(link_type, unsplit_target, _)) = event {
                // Check inline links only (not URLs or e-mail addresses in autolinks for instance)
                if let LinkType::Inline = link_type {
                    // Get the link's target file and optionally its header
                    let (target, header): (String, Option<String>) =
                        match unsplit_target.chars().position(|c| c == '#') {
                            Some(index) => (
                                unsplit_target.chars().take(index).collect(),
                                Some(unsplit_target.chars().skip(index + 1).collect()),
                            ),
                            None => (unsplit_target.into_string(), None),
                        };

                    // Don't care about URLs
                    if target.starts_with("http://")
                        || target.starts_with("https://")
                        || target.starts_with("ftp://")
                    {
                        trace!("{}", format_msg!("found link to URL: {}", target));
                        continue;
                    }

                    if EMAIL_REGEX.is_match(&target) {
                        trace!("{}", format_msg!("found link to e-mail addres: {}", target));
                        continue;
                    }

                    let target = if !target.is_empty() {
                        path.parent().unwrap().join(Path::new(&target))
                    } else {
                        path.to_owned()
                    };

                    let target_canon = safe_canonicalize(&target);

                    if !target.exists() {
                        err_or_warn!(
                            "{}",
                            format_msg!(
                                "broken link found: path '{}' does not exist",
                                target_canon.green()
                            )
                        );
                        errors += 1;
                    } else {
                        trace!("{}", format_msg!("valid link found: {}", target_canon));

                        // If header links must be checked...
                        if !ignore_header_links {
                            // If the link points to a specific header...
                            if let Some(header) = header {
                                // Then the target must be a file
                                if !target.is_file() {
                                    err_or_warn!("{}", format_msg!("invalid header link found: path '{}' exists but is not a file", target_canon.green()));
                                    errors += 1;
                                } else {
                                    debug!(
                                        "{}",
                                        format_msg!(
                                            "now checking link '{}' from file '{}'",
                                            header,
                                            target_canon
                                        )
                                    );

                                    // Canonicalize properly the target path to avoid irregularities in cache's keys
                                    //  like 'dir/../file.md' and 'file.md' which are identical but do not have the same Path representation
                                    let unified_target = target.canonicalize().unwrap();

                                    // If the target file is not already in cache...
                                    if !links_cache.contains_key(&unified_target) {
                                        // 2. Push all slugs in the cache
                                        links_cache.insert(
                                            unified_target.clone(),
                                            // 1. Get all its headers as slugs
                                            // We do not use the fully canonicalized path to not force displaying an absolute path
                                            generate_slugs(&target).map_err(|err| {
                                                format!(
                                                    "failed to generate slugs for file '{}': {}",
                                                    target_canon.green(),
                                                    err
                                                )
                                            })?,
                                        );
                                    }

                                    // Get the file's slugs from the cache
                                    let slugs = links_cache.get(&unified_target).unwrap();

                                    // Ensure the link points to an existing header
                                    if !slugs.contains(&header) {
                                        err_or_warn!(
                                            "{}",
                                            format_msg!(
                                                "broken link found: header '{}' not found in '{}'",
                                                header.yellow(),
                                                target_canon.green()
                                            )
                                        );
                                        errors += 1;
                                    } else {
                                        trace!(
                                            "{}",
                                            format_msg!("valid header link found: {}", header)
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Everything went fine :D
    Ok(errors)
}
