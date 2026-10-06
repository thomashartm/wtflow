use anyhow::Result;
use std::{
    io::{self, BufRead, IsTerminal, Write},
    path::Path,
};

mod terminal;

pub fn select(
    input: &mut impl BufRead,
    root: &Path,
    title: &str,
    items: &[Item],
    allow_new: bool,
    initial_query: &str,
) -> Result<Option<usize>> {
    if io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var("TERM").as_deref() != Ok("dumb")
    {
        terminal::choose(root, title, items, allow_new, initial_query)
    } else {
        choose(
            input,
            &mut io::stdout().lock(),
            title,
            items,
            allow_new,
            initial_query,
        )
    }
}

const PAGE_SIZE: usize = 8;

pub struct Item {
    pub title: String,
    pub symbol: String,
    pub path: String,
    pub is_test: bool,
}

pub fn is_test_file(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.split('/').any(|part| {
        matches!(
            part,
            "test"
                | "tests"
                | "__tests__"
                | "__mocks__"
                | "testdata"
                | "fixtures"
                | "__fixtures__"
                | "snapshots"
        )
    }) || path.rsplit('/').next().is_some_and(|name| {
        name.contains(".spec.")
            || name.contains(".test.")
            || name.contains(".e2e-spec.")
            || name.starts_with("test_")
            || name.ends_with("_test.py")
            || name.ends_with("Test.java")
            || name.ends_with("Tests.java")
    })
}

/// Numbers refer to the original sorted inventory, even after paging/filtering.
/// Only visible numbers may be selected, so hidden matches cannot be chosen by mistake.
pub fn choose(
    input: &mut impl BufRead,
    output: &mut impl Write,
    title: &str,
    items: &[Item],
    allow_new: bool,
    initial_query: &str,
) -> Result<Option<usize>> {
    let mut query = initial_query.to_owned();
    let mut page = 0usize;
    let mut include_tests = false;
    let test_count = items.iter().filter(|item| item.is_test).count();
    loop {
        let filter = crate::filter::Filter::new(&query);
        let matches: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                (include_tests || !item.is_test)
                    && filter.matches(&[&item.title, &item.symbol, &item.path])
            })
            .collect();
        let pages = matches.len().div_ceil(PAGE_SIZE).max(1);
        page = page.min(pages - 1);
        let visible: Vec<_> = matches
            .iter()
            .skip(page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect();
        writeln!(
            output,
            "\n{title} | {} matches | page {}/{}",
            matches.len(),
            page + 1,
            pages
        )?;
        if !query.is_empty() {
            writeln!(output, "Search: {query}")?;
        }
        if test_count > 0 {
            writeln!(
                output,
                "Tests: {} ({test_count})",
                if include_tests { "shown" } else { "hidden" }
            )?;
        }
        writeln!(output)?;
        for (index, item) in &visible {
            writeln!(
                output,
                "  {:>3}. {}{}",
                index + 1,
                item.title,
                if item.is_test { " [test]" } else { "" }
            )?;
            writeln!(output, "       {}", item.symbol)?;
            writeln!(output, "       {}\n", item.path)?;
        }
        if visible.is_empty() {
            writeln!(
                output,
                "  No matches. Clear the search with / or toggle tests with t.\n"
            )?;
        }
        writeln!(
            output,
            "  /text search   / clear   > next   < previous   t tests   q quit"
        )?;
        if allow_new {
            writeln!(output, "  n analyze a new flow")?;
        }
        write!(output, "Choose a displayed number or command: ")?;
        output.flush()?;
        let mut answer = String::new();
        if input.read_line(&mut answer)? == 0 {
            return Ok(None);
        }
        let answer = answer.trim();
        match answer {
            "q" | "Q" => return Ok(None),
            "n" | "N" if allow_new => return Ok(Some(items.len())),
            ">" => page = (page + 1).min(pages - 1),
            "<" => page = page.saturating_sub(1),
            "t" | "T" => {
                include_tests = !include_tests;
                page = 0;
            }
            _ if answer.starts_with('/') => {
                query = answer[1..].trim().to_lowercase();
                page = 0;
            }
            _ => {
                if let Ok(number) = answer.parse::<usize>() {
                    if let Some((index, _)) = visible.iter().find(|(index, _)| index + 1 == number)
                    {
                        return Ok(Some(*index));
                    }
                }
                writeln!(
                    output,
                    "\nPlease choose a displayed number, or use /text to search."
                )?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn items() -> Vec<Item> {
        (1..=19)
            .map(|n| Item {
                title: format!("http POST /orders/{n}"),
                symbol: format!("Orders.action{n}"),
                path: format!("src/orders{n}.ts"),
                is_test: n == 19,
            })
            .collect()
    }
    fn pick(items: &[Item], input: &str, allow_new: bool) -> (Option<usize>, String) {
        let mut output = Vec::new();
        let selected = choose(
            &mut input.as_bytes(),
            &mut output,
            "Entrypoints",
            items,
            allow_new,
            "",
        )
        .unwrap();
        (selected, String::from_utf8(output).unwrap())
    }
    #[test]
    fn pages_search_and_test_toggle_preserve_selection_identity() {
        let items = items();
        let (_, first) = pick(&items, "q\n", false);
        assert!(first.contains("18 matches | page 1/3"));
        assert!(!first.contains("Orders.action9"));
        assert!(!first.contains("Orders.action19"));
        assert_eq!(pick(&items, ">\n9\n", false).0, Some(8));
        assert_eq!(pick(&items, ">\n<\n1\n", false).0, Some(0));
        assert_eq!(pick(&items, "/ORDERS18.ts\n18\n", false).0, Some(17));
        assert_eq!(pick(&items, "/action19\nt\n19\n", false).0, Some(18));
        let (selected, output) = pick(&items, "9\n/unknown\n1\n/\n1\n", false);
        assert_eq!(selected, Some(0));
        assert!(output.contains("No matches."));
        assert!(output.contains("Please choose a displayed number"));
        assert_eq!(pick(&items, "n\n", true).0, Some(items.len()));
        assert_eq!(pick(&items, "", false).0, None);
    }
    #[test]
    fn recognizes_test_paths_without_hiding_application_filenames() {
        for path in [
            "src/order.spec.ts",
            "src/order.test.tsx",
            "test/fixture.ts",
            "tests/app.py",
            "src/__tests__/controller.ts",
            "src/order.e2e-spec.ts",
            "tests\\helper.ts",
            "test_app.py",
            "src/test/java/AppTest.java",
        ] {
            assert!(is_test_file(path), "{path}");
        }
        for path in [
            "src/testing.service.ts",
            "src/latest/orders.ts",
            "src/testimonials.ts",
            "src/contest.py",
        ] {
            assert!(!is_test_file(path), "{path}");
        }
    }
}
