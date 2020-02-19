//! A library and command-line tool for detecting broken links in Markdown files
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
//! Detailed informations can be displayed with `-v verbose`.
//! Debug informations can be displayed with `-v trace`.
//! Informations messages and warnings can be hidden to only show errors with `-v errors`.
//! Output can be turned off with `-v silent` (exit code will be 0 if there was no broken link).
//! 
//! ## Library usage
//! 
//! ```
//! use broken_md_links::check_broken_links;
//! 
//! fn main() {
//!   match check_broken_links(Path::new("file.md"), false, false, &mut HashMap::new()) {
//!     Ok(0) => println!("No broken link :D"),
//!     Ok(errors @ _) => println!("There are {} broken links :(", errors),
//!     Err(err) => println!("Something went wrong :( : {}", err)
//!   }
//! }
//! ```

use std::path::{Path, PathBuf, Component};
use std::collections::HashMap;
use log::{trace, debug, info, warn, error};
use pulldown_cmark::{Parser, Options, Event, Tag, LinkType};

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
            Component::CurDir => {},

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
    out.iter().collect::<PathBuf>().to_string_lossy().into_owned()
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
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
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
        .map_err(|err| format!("Failed to read file at '{}': {}", canon, err))?;

    trace!("In '{}': just read file, which is {} bytes long.", canon, content.len());

    // The list of slugified headers
    let mut headers = vec![];

    // When the 'pulldown_cmark' library encounters a heading, the title is only made available in the next event
    // This boolean is set to `true` when an heading appears, to indicate the next event is expected to be the related title
    let mut get_header = false;

    // Create a pull-down markdown parser
    let parser = Parser::new_ext(&content, Options::all());

    for event in parser {
        // If the last event was an heading, we are now expecting to get its title
        if get_header {
            // If we indeed get a piece of text (not a paragraph)...
            if let Event::Text(header) = &event {
                // Get its slug and push it to the list of this file's headers
                let slug = slugify(&header);
                debug!("In '{}': found header: #{}", canon, slug);
                headers.push(slug);
            } else {
                // We did not get a piece of text, which means this heading does not have a title
                warn!("In '{}': heading was not directly followed by a title", canon);
                trace!("Faulty event: {:?}", event);
            }

            // Disable the title expectation
            get_header = false;

        }

        // If we encounted an heading...
        if let Event::Start(Tag::Heading(_)) = event {
            // Expect to get the related title just after
            get_header = true;
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
pub fn check_broken_links(path: &Path, dir: bool, ignore_header_links: bool, mut links_cache: &mut HashMap<PathBuf, Vec<String>>) -> Result<u64, String> {
    // Get the canonicalized path for display
    let canon = safe_canonicalize(path);

    debug!("Analyzing directory: {}", canon);

    // Count errors
    let mut errors = 0;

    if dir {
        // Treat input as a directory

        for item in path.read_dir().map_err(|err| format!("Failed to read input directory at '{}': {}", canon, err))? {
            let item = item.map_err(|err| format!("Failed to get item from directory at '{}': {}", canon, err))?;
            let path = item.path();
            let file_type = item.file_type().map_err(|err| format!("Failed to read file type of item at '{}': {}", canon, err))?;

            if file_type.is_dir() {
                // Check broken links recursively
                errors += check_broken_links(&path, true, ignore_header_links, &mut links_cache)?;
            } else if file_type.is_file() {
                // Only check ".md" files
                if let Some(ext) = path.extension() {
                    if let Some(ext) = ext.to_str() {
                        if ext == "md" {
                            // Check this Markdown file
                            errors += check_broken_links(&path, false, ignore_header_links, links_cache)?;
                        }
                    }
                }
            } else {
                warn!("Item at path '{}' is neither a file nor a directory so it will be ignored", canon);
            }
        }
    } else {
        // Treat input as a file
        info!("Analyzing: {}", canon);

        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("Failed to read file at '{}': {}", canon, err))?;

        trace!("In '{}': just read file, which is {} bytes long.", canon, content.len());

        // Count links without a target (like `[link name]`) as an error
        // TODO: Find a way to not have to specify such a long explicit type name
        let handle_missing_link_targets: &dyn for<'r, 's> Fn(&'r str, &'s str) -> Option<(String, String)> = &|link, _| {
            error!("In '{}': Missing target for link '{}'", canon, link);
            None
        };

        // Create a pull-down parser
        let parser = Parser::new_with_broken_link_callback(&content, Options::all(), Some(handle_missing_link_targets));

        for event in parser {
            // Check links only
            if let Event::End(Tag::Link(link_type, unsplit_target, _)) = event {
                // Check inline links only (not URLs or e-mail addresses for instance)
                if let LinkType::Inline = link_type {
                    // Get the link's target file and optionally its header
                    let (target, header): (String, Option<String>) = match unsplit_target.chars().position(|c| c == '#') {
                        Some(index) => (
                            unsplit_target.chars().take(index).collect(),
                            Some(unsplit_target.chars().skip(index + 1).collect())
                        ),
                        None => (unsplit_target.into_string(), None)
                    };

                    let target = path.parent().unwrap().join(Path::new(&target));
                    let target_canon = safe_canonicalize(&target);

                    if !target.exists() {
                        error!("In '{}': Broken link found: path '{}' does not exist", canon, target_canon);
                        errors += 1;
                    } else {
                        trace!("In '{}': valid link found: {}", canon, target_canon);

                        // If header links must be checked...
                        if !ignore_header_links {
                            // If the link points to a specific header...
                            if let Some(header) = header {
                                // Then the target must be a file
                                if !target.is_file() {
                                    error!("In '{}': Invalid header link found: path '{}' exists but is not a file", canon, target_canon);
                                    errors += 1;
                                } else {
                                    debug!("In '{}': now checking link '{}' from file '{}'", canon, header, target_canon);
                                
                                    // If the target file is not already in cache...
                                    if !links_cache.contains_key(&target) {
                                        // 2. Push all slugs in the cache
                                        links_cache.insert(target.clone(),
                                            // 1. Get all its headers as slugs
                                            generate_slugs(&target)
                                                .map_err(|err| format!("Failed to generate slugs for file '{}': {}", target_canon, err))?
                                        );
                                    }

                                    // Get the file's slugs from the cache
                                    let slugs = links_cache.get(&target).unwrap();

                                    // Ensure the link points to an existing header
                                    if !slugs.contains(&header) {
                                        error!("In '{}': Broken link found: header '{}' not found in '{}'", canon, header, target_canon);
                                    } else {
                                        trace!("In '{}': valid header link found: {}", canon, header);
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
