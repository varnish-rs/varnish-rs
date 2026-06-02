use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use varnish::{Metric, MetricFormat, MetricsReaderBuilder, Semantics};

#[derive(Parser)]
#[command(about = "Display Varnish statistics as a tree")]
struct Cli {
    /// Varnish instance name (working directory)
    #[arg(short = 'n')]
    name: Option<String>,

    /// Attach timeout in seconds
    #[arg(short = 't', default_value = "5.0")]
    timeout: f64,

    /// Field inclusion glob, passed to VSC (may be repeated, processed before -X)
    #[arg(short = 'I')]
    include: Vec<String>,

    /// Field exclusion glob, passed to VSC (may be repeated, processed after -I)
    #[arg(short = 'X')]
    exclude: Vec<String>,

    /// Show all metrics, including those with a value of 0
    #[arg(short = 'a')]
    all: bool,

    /// Show format and short description columns
    #[arg(short = 'v')]
    verbose: bool,
}

struct Node {
    children: HashMap<String, Node>,
    value: Option<u64>,
    short_desc: String,
    format: MetricFormat,
}

impl Default for Node {
    fn default() -> Self {
        Node {
            children: HashMap::new(),
            value: None,
            short_desc: String::new(),
            format: MetricFormat::Unknown,
        }
    }
}

impl Node {
    fn new() -> Self {
        Self::default()
    }
}

fn metric_value(metric: &Metric<'_>) -> u64 {
    if metric.semantics == Semantics::Gauge {
        metric.get_clamped_value()
    } else {
        metric.get_raw_value()
    }
}

fn insert(root: &mut Node, parts: &[&str], metric: &Metric<'_>) {
    if parts.is_empty() {
        root.value = Some(metric_value(metric));
        root.short_desc = metric.short_desc.to_string();
        root.format = metric.format;
        return;
    }
    let child = root
        .children
        .entry(parts[0].to_string())
        .or_insert_with(Node::new);
    insert(child, &parts[1..], metric);
}

fn format_value(node: &Node) -> String {
    let value = node.value.unwrap_or(0);
    if node.format == MetricFormat::Bitmap {
        format!("0x{value:016x}")
    } else {
        value.to_string()
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

    if !cli.timeout.is_finite() || cli.timeout < 0.0 {
        eprintln!("invalid timeout: must be a non-negative finite number");
        std::process::exit(1);
    }

    let mut builder =
        MetricsReaderBuilder::new().patience(Some(Duration::from_secs_f64(cli.timeout)));

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
        if !cli.all && metric_value(metric) == 0 {
            continue;
        }
        let parts: Vec<&str> = metric.name.split('.').collect();
        insert(&mut root, &parts, metric);
    }

    let top_keys = sorted_keys(&root.children);
    for k in top_keys {
        println!("{k}");
        print_children(&root.children[k], "", cli.verbose);
    }
}
