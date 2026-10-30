//! Skill discovery and prompt injection. pi-web gets this from the pi SDK's
//! resource loader; the Rust agent reads the same `SKILL.md` folders.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::settings;

const MAX_SKILL_TOTAL_CHARS: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub content: String,
    pub disabled: bool,
    pub scope: &'static str,
}

fn parse_frontmatter(text: &str) -> Option<(String, String, bool)> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name = None;
    let mut description = None;
    let mut disabled = false;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(
                value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if line.starts_with("disable-model-invocation:") {
            disabled = line.ends_with("true");
        }
    }
    Some((name?, description.unwrap_or_default(), disabled))
}

fn roots(config: &Config, cwd: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut roots = vec![
        (config.agent_dir.join("skills"), "global"),
        (
            settings::home_dir()
                .unwrap_or_default()
                .join(".agents")
                .join("skills"),
            "global",
        ),
    ];
    if settings::project_trust(config, cwd).trusted {
        roots.push((cwd.join(".pi").join("skills"), "project"));
        roots.push((cwd.join(".agents").join("skills"), "project"));
    }
    roots
}

pub fn discover(config: &Config, cwd: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();
    for (root, scope) in roots(config, cwd) {
        if !root.is_dir() {
            continue;
        }
        for entry in ignore::WalkBuilder::new(&root)
            .max_depth(Some(3))
            .build()
            .flatten()
        {
            if !entry.file_type().is_some_and(|kind| kind.is_file())
                || entry.path().file_name().and_then(|name| name.to_str()) != Some("SKILL.md")
            {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let (name, description, disabled) = parse_frontmatter(&content).unwrap_or_else(|| {
                (
                    entry
                        .path()
                        .parent()
                        .and_then(Path::file_name)
                        .and_then(|name| name.to_str())
                        .unwrap_or("skill")
                        .to_string(),
                    String::new(),
                    false,
                )
            });
            skills.push(Skill {
                name,
                description,
                file_path: entry.path().to_path_buf(),
                content,
                disabled,
                scope,
            });
        }
    }
    skills
}

/// The system-prompt section for every enabled skill, or an empty string.
pub fn prompt_section(config: &Config, cwd: &Path) -> String {
    let enabled: Vec<Skill> = discover(config, cwd)
        .into_iter()
        .filter(|skill| !skill.disabled)
        .collect();
    if enabled.is_empty() {
        return String::new();
    }
    let mut section = String::from(
        "\n\n# Skills\n\nThe following skills are available. Follow a skill's instructions when the user's request matches its description.\n",
    );
    let mut used = section.chars().count();
    for skill in enabled {
        if used >= MAX_SKILL_TOTAL_CHARS {
            break;
        }
        let mut block = format!("\n## {}\n", skill.name);
        if !skill.description.is_empty() {
            block.push_str(&format!("\n{}\n", skill.description));
        }
        block.push_str(&format!("\nSource: {}\n\n", skill.file_path.display()));
        block.push_str(skill.content.trim());
        block.push('\n');
        let remaining = MAX_SKILL_TOTAL_CHARS.saturating_sub(used);
        if block.chars().count() > remaining {
            block = block.chars().take(remaining).collect();
        }
        used += block.chars().count();
        section.push_str(&block);
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_section_injects_enabled_skills_only() {
        let temp = tempfile::tempdir().unwrap();
        let agent = temp.path().join("agent");
        let cwd = temp.path().join("project");
        let skills = agent.join("skills");
        std::fs::create_dir_all(skills.join("review")).unwrap();
        std::fs::create_dir_all(skills.join("hidden")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            skills.join("review").join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\n\nReview the diff carefully.\n",
        )
        .unwrap();
        std::fs::write(
            skills.join("hidden").join("SKILL.md"),
            "---\nname: hidden\ndescription: Hidden\ndisable-model-invocation: true\n---\n\nNot injected.\n",
        )
        .unwrap();
        let config = Config::new(Some(agent), None).unwrap();
        let section = prompt_section(&config, &cwd);
        assert!(section.contains("## review"));
        assert!(section.contains("Review the diff carefully."));
        assert!(!section.contains("## hidden"));
        assert!(!section.contains("Not injected."));
    }

    #[test]
    fn frontmatter_controls_name_description_and_disabling() {
        let (name, description, disabled) = parse_frontmatter(
            "---\nname: review\ndescription: Review code\ndisable-model-invocation: true\n---\nbody",
        )
        .unwrap();
        assert_eq!(name, "review");
        assert_eq!(description, "Review code");
        assert!(disabled);
    }
}
