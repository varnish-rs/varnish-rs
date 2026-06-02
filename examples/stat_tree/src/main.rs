use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use varnish::{MetricFormat, MetricsReaderBuilder, Semantics};

#[derive(Parser)]
#[command(about = "Display Varnish statistics as a tree")]
struct Cli {
    /// Varnish instance name (working directory)
    #[arg(short = 'n')]
    name: Option<String>,

    /// Attach timeout in seconds
    #[arg(short = 't')]
    timeout: Option<f64>,

    /// Field inclusion glob, passed to VSC (may be repeated, processed before -X)
    #[arg(short = 'I')]
    include: Vec<String>,

    /// Field exclusion glob, passed to VSC (may be repeated, processed after -I)
    #[arg(short = 'X')]
    exclude: Vec<String>,

    /// Show all metrics, including those with a value of 0
    #[arg(short = 'a')]
    all: bool,

    /// Show short description next to each value
    #[arg(short = 'v')]
    verbose: bool,
}

struct Node {
    children: HashMap<String, Node>,
    value: Option<u64>,
    short_desc: String,
    semantics: Semantics,
    format: MetricFormat,
}

impl Node {
    fn new() -> Self {
        Node {
            children: HashMap::new(),
            value: None,
            short_desc: String::new(),
            semantics: Semantics::Unknown,
            format: MetricFormat::Unknown,
        }
    }
}

fn insert(
    root: &mut Node,
    parts: &[&str],
    value: u64,
    short_desc: &str,
    semantics: Semantics,
    format: MetricFormat,
) {
    if parts.is_empty() {
        root.value = Some(value);
        root.short_desc = short_desc.to_string();
        root.semantics = semantics;
        root.format = format;
        return;
    }
    let child = root
        .children
        .entry(parts[0].to_string())
        .or_insert_with(Node::new);
    insert(child, &parts[1..], value, short_desc, semantics, format);
}

fn format_value(node: &Node) -> String {
    let value = node.value.unwrap_or(0);
    if node.format == MetricFormat::Bitmap {
        format!("0x{value:016x}")
    } else {
        format!("{value}")
    }
}

fn sorted_keys(children: &HashMap<String, Node>) -> Vec<&str> {
    let mut keys: Vec<&str> = children.keys().map(String::as_str).collect();
    keys.sort_by(|&a, &b| {
        let is_section_a = !children[a].children.is_empty();
        let is_section_b = !children[b].children.is_empty();
        is_section_b.cmp(&is_section_a).then(a.cmp(b))
    });
    keys
}

fn format_label(format: MetricFormat) -> &'static str {
    match format {
        MetricFormat::Integer => "integer",
        MetricFormat::Bytes => "bytes",
        MetricFormat::Bitmap => "bitmap",
        MetricFormat::Duration => "duration",
        MetricFormat::Unknown => "?",
    }
}

fn print_children(node: &Node, prefix: &str, verbose: bool) {
    let keys = sorted_keys(&node.children);

    let leaves: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|&k| node.children[k].value.is_some())
        .collect();

    let max_key = leaves.iter().map(|k| k.len()).max().unwrap_or(0);
    let max_val = leaves
        .iter()
        .map(|&k| format_value(&node.children[k]).len())
        .max()
        .unwrap_or(0);
    let max_label = leaves
        .iter()
        .map(|&k| format_label(node.children[k].format).len())
        .max()
        .unwrap_or(0);

    let n = keys.len();
    for (i, &k) in keys.iter().enumerate() {
        let child = &node.children[k];
        let is_last = i == n - 1;
        let (connector, next_prefix) = if is_last {
            ("└── ", format!("{prefix}    "))
        } else {
            ("├── ", format!("{prefix}│   "))
        };

        if child.value.is_some() {
            let val = format_value(child);
            let pad = max_key - k.len() + max_val;
            if verbose {
                let label = format_label(child.format);
                println!(
                    "{prefix}{connector}{k}: {val:>pad$}  {label:<max_label$}  {}",
                    child.short_desc
                );
            } else {
                println!("{prefix}{connector}{k}: {val:>pad$}");
            }
        } else {
            println!("{prefix}{connector}{k}");
        }

        print_children(child, &next_prefix, verbose);
    }
}

fn main() {
    let cli = Cli::parse();

    let mut builder = MetricsReaderBuilder::new();
    if let Some(t) = cli.timeout {
        builder = builder.patience(Some(Duration::from_secs_f64(t)));
    }

    for pattern in &cli.include {
        builder = builder.include(pattern).unwrap_or_else(|e| {
            eprintln!("invalid include pattern '{pattern}': {e}");
            std::process::exit(1);
        });
    }
    for pattern in &cli.exclude {
        builder = builder.exclude(pattern).unwrap_or_else(|e| {
            eprintln!("invalid exclude pattern '{pattern}': {e}");
            std::process::exit(1);
        });
    }

    if let Some(ref n) = cli.name {
        builder = builder.work_dir(Path::new(n)).unwrap_or_else(|e| {
            eprintln!("invalid work dir: {e}");
            std::process::exit(1);
        });
    }

    let mut reader = builder.build().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    reader.update();

    let mut root = Node::new();
    for metric in reader.stats().values() {
        if !cli.all && metric.get_raw_value() == 0 {
            continue;
        }
        let parts: Vec<&str> = metric.name.split('.').collect();
        insert(
            &mut root,
            &parts,
            metric.get_raw_value(),
            metric.short_desc,
            metric.semantics,
            metric.format,
        );
    }

    let top_keys = sorted_keys(&root.children);
    for k in top_keys {
        println!("{k}");
        print_children(&root.children[k], "", cli.verbose);
    }
}
