//! List rules command implementation.

use arch_lint_rules::all_rules;

/// Runs the list-rules command.
pub fn run() {
    println!("Available rules:\n");
    println!("{:<10} {:<25} Description", "Code", "Name");
    println!("{}", "-".repeat(80));

    for rule in all_rules() {
        println!(
            "{:<10} {:<25} {}",
            rule.code(),
            rule.name(),
            rule.description()
        );
    }

    println!("\nPresets:");
    println!("  recommended  - AL001, AL002, AL003, AL005, AL006, AL007 (default)");
    println!("  strict       - All rules with stricter settings");
    println!("  minimal      - AL001 only (for gradual adoption)");

    println!("\nUse --rules to filter specific rules, e.g.:");
    println!("  arch-lint check --rules no-unwrap-expect,no-sync-io");
    println!("  arch-lint check --rules AL001,AL002,AL003");
}
