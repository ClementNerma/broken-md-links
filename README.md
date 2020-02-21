# Broken Markdown Links

This repository is an utility written in Rust that ensures all links in a Markdown file are valid, by ensuring the target files exist.
It also ensures that for links pointing to a specific header (like `[link name](file.md#some-header)`) the said header exists in the target file.

## Command-line usage

Check a single file:

```shell
broken-md-links input.md
```

Check a whole directory:

```shell
broken-md-links dir/ -r
```

### Output

There are several levels of verbosity:

* `-v silent`: display nothing (exit code will be 0 if there was no broken link)
* `-v errors`: display errors only
* `-v warn`: display errors and warnings (the default)
* `-v info`: display the list of analyzed files as well
* `-v verbose`: display detailed informations
* `-v trace`: display debug informations

## Library usage

```rust
use broken_md_links::check_broken_links;

fn main() {
  match check_broken_links(Path::new("file.md"), false, false, false, &mut HashMap::new()) {
    Ok(0)          => println!("No broken link :D"),
    Ok(errors @ _) => println!("There are {} broken links :(", errors),
    Err(err)       => println!("Something went wrong :( : {}", err)
  }
}
```

## License

This project is released under the [Apache-2.0](LICENSE.md) license terms.
