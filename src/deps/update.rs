use crate::core::{AppError, DepInfo, Ecosystem, Result};
use crate::deps::DepsParser;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use toml_edit::DocumentMut;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateAction {
    Command {
        program: String,
        args: Vec<String>,
    },
    Manifest {
        path: PathBuf,
        format: ManifestFormat,
    },
    ManifestThenCommand {
        path: PathBuf,
        format: ManifestFormat,
        program: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestFormat {
    Requirements,
    Pyproject,
    Gemfile,
}

#[derive(Debug, Clone)]
pub struct UpdatePlan {
    pub project_path: PathBuf,
    pub package: String,
    pub ecosystem: Ecosystem,
    pub current_version: String,
    pub target_version: String,
    pub action: UpdateAction,
}

impl UpdatePlan {
    pub fn command_line(&self) -> String {
        match &self.action {
            UpdateAction::Command { program, args } => {
                format!("{} {}", program, args.join(" "))
            }
            UpdateAction::Manifest { path, .. } => format!("Edit {}", path.display()),
            UpdateAction::ManifestThenCommand {
                path,
                program,
                args,
                ..
            } => format!("Edit {} && {} {}", path.display(), program, args.join(" ")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateOutcome {
    pub output: String,
}

pub struct UpdateManager;

impl UpdateManager {
    pub fn plan(project_path: &Path, dependency: &DepInfo) -> Result<UpdatePlan> {
        if !dependency.update_available {
            return Err(AppError::Other(format!(
                "{} is already up to date",
                dependency.name
            )));
        }
        let target = dependency
            .latest_version
            .clone()
            .filter(|version| !version.trim().is_empty())
            .ok_or_else(|| AppError::Other(format!("{} has no target version", dependency.name)))?;
        let package = dependency.name.clone();
        let action = match dependency.ecosystem {
            Ecosystem::Npm => {
                let mut args = vec!["install".into(), format!("{package}@{target}")];
                if dependency.dev_dependency {
                    args.push("--save-dev".into());
                }
                command("npm", args)
            }
            Ecosystem::Cargo => {
                let mut args = vec!["add".into(), format!("{package}@{target}")];
                if dependency.dev_dependency {
                    args.push("--dev".into());
                }
                if let Some(alias) = &dependency.manifest_name {
                    args.extend(["--rename".into(), alias.clone()]);
                }
                command("cargo", args)
            }
            Ecosystem::Go => command(
                "go",
                vec![
                    "get".into(),
                    format!("{}@v{}", package, target.trim_start_matches('v')),
                ],
            ),
            Ecosystem::Composer => {
                let mut args = vec![
                    "require".into(),
                    format!("{package}:{}", target.trim_start_matches('v')),
                    "--with-all-dependencies".into(),
                    "--no-interaction".into(),
                ];
                if dependency.dev_dependency {
                    args.push("--dev".into());
                }
                command("composer", args)
            }
            Ecosystem::Gem => UpdateAction::ManifestThenCommand {
                path: project_path.join("Gemfile"),
                format: ManifestFormat::Gemfile,
                program: "bundle".into(),
                args: vec!["update".into(), package.clone()],
            },
            Ecosystem::Pip => {
                let pyproject = project_path.join("pyproject.toml");
                let path = if pyproject_declares_dependency(&pyproject, &package) {
                    pyproject
                } else {
                    project_path.join("requirements.txt")
                };
                let format = if path.ends_with("pyproject.toml") {
                    ManifestFormat::Pyproject
                } else {
                    ManifestFormat::Requirements
                };
                if !path.exists() {
                    return Err(AppError::Config(format!(
                        "No Python dependency manifest found for {package}"
                    )));
                }
                UpdateAction::Manifest { path, format }
            }
        };

        Ok(UpdatePlan {
            project_path: project_path.to_path_buf(),
            package,
            ecosystem: dependency.ecosystem,
            current_version: dependency.current_version.clone(),
            target_version: target,
            action,
        })
    }

    pub fn execute(plan: &UpdatePlan) -> Result<UpdateOutcome> {
        Self::execute_with_cancellation(plan, None)
    }

    pub fn execute_with_cancellation(
        plan: &UpdatePlan,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<UpdateOutcome> {
        if cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err(AppError::Other("Update cancelled by user".into()));
        }
        match &plan.action {
            UpdateAction::Command { program, args } => {
                Self::execute_command(plan, program, args, cancel)
            }
            UpdateAction::Manifest { path, format } => Self::execute_manifest(plan, path, *format),
            UpdateAction::ManifestThenCommand {
                path,
                format,
                program,
                args,
            } => Self::execute_manifest_then_command(plan, path, *format, program, args, cancel),
        }
    }

    fn execute_command(
        plan: &UpdatePlan,
        program: &str,
        args: &[String],
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<UpdateOutcome> {
        let mut command = Command::new(program);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .args(args)
            .current_dir(&plan.project_path)
            .stdin(Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| {
                AppError::Other(format!("Cannot run '{}': {error}", plan.command_line()))
            })?;
        let stdout_thread = child.stdout.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                buffer
            })
        });
        let stderr_thread = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                buffer
            })
        });
        let started = Instant::now();
        let status = loop {
            if cancel
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                kill_process_group(&mut child);
                let _ = child.wait();
                return Err(AppError::Other("Update cancelled by user".into()));
            }
            if started.elapsed() >= Duration::from_secs(300) {
                kill_process_group(&mut child);
                let _ = child.wait();
                return Err(AppError::Other("Update timed out after 300 seconds".into()));
            }
            if let Some(status) = child.try_wait()? {
                break status;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let _ = child.wait_with_output()?;
        let stdout = stdout_thread
            .map(|thread| thread.join().unwrap_or_default())
            .map(|output| String::from_utf8_lossy(&output).trim().to_string())
            .unwrap_or_default();
        let stderr = stderr_thread
            .map(|thread| thread.join().unwrap_or_default())
            .map(|output| String::from_utf8_lossy(&output).trim().to_string())
            .unwrap_or_default();
        if !status.success() {
            let detail = if stderr.is_empty() { stdout } else { stderr };
            return Err(AppError::Other(format!(
                "'{}' failed with exit status {}: {}",
                plan.command_line(),
                status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "terminated by signal".into()),
                detail
            )));
        }
        Ok(UpdateOutcome {
            output: if stdout.is_empty() { stderr } else { stdout },
        })
    }

    fn execute_manifest_then_command(
        plan: &UpdatePlan,
        path: &Path,
        format: ManifestFormat,
        program: &str,
        args: &[String],
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<UpdateOutcome> {
        let original = std::fs::read_to_string(path)?;
        match Self::execute_manifest(plan, path, format)
            .and_then(|_| Self::execute_command(plan, program, args, cancel))
        {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                Self::write_atomic(path, &original)?;
                Err(error)
            }
        }
    }

    fn execute_manifest(
        plan: &UpdatePlan,
        path: &Path,
        format: ManifestFormat,
    ) -> Result<UpdateOutcome> {
        let content = std::fs::read_to_string(path)?;
        let updated = match format {
            ManifestFormat::Requirements => Self::update_requirements(&content, plan)?,
            ManifestFormat::Pyproject => Self::update_pyproject(&content, plan)?,
            ManifestFormat::Gemfile => Self::update_gemfile(&content, plan)?,
        };
        Self::write_atomic(path, &updated)?;

        let verification = (|| {
            let verified = DepsParser::detect_and_parse(&plan.project_path)
                .into_iter()
                .find(|dependency| {
                    dependency.name == plan.package && dependency.ecosystem == plan.ecosystem
                })
                .ok_or_else(|| {
                    AppError::Other(format!(
                        "{} was not found after updating {}",
                        plan.package,
                        path.display()
                    ))
                })?;
            if verified.current_version != plan.target_version {
                return Err(AppError::Other(format!(
                    "Update verification failed for {}: expected {}, found {}",
                    plan.package, plan.target_version, verified.current_version
                )));
            }
            Ok(())
        })();
        if let Err(error) = verification {
            Self::write_atomic(path, &content)?;
            return Err(error);
        }

        Ok(UpdateOutcome {
            output: format!(
                "Updated {}=={} in {}",
                plan.package,
                plan.target_version,
                path.display()
            ),
        })
    }

    fn update_requirements(content: &str, plan: &UpdatePlan) -> Result<String> {
        for line in content.lines() {
            let (specification, _) = split_trailing_comment(line);
            if normalize_package_name(requirement_name(specification.trim_start()))
                == normalize_package_name(&plan.package)
            {
                Self::validate_python_spec(specification)?;
            }
        }
        let mut replacements = 0;
        let updated = content
            .split_inclusive('\n')
            .map(|line| {
                let body = line.trim_end_matches(['\r', '\n']);
                let ending = &line[body.len()..];
                let (specification, comment) = split_trailing_comment(body);
                let trimmed = specification.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
                    return line.to_string();
                }
                if normalize_package_name(requirement_name(trimmed))
                    != normalize_package_name(&plan.package)
                {
                    return line.to_string();
                }
                replacements += 1;
                let indent = &specification[..specification.len() - trimmed.len()];
                let suffix = if comment.is_empty() {
                    String::new()
                } else {
                    format!(" {comment}")
                };
                format!(
                    "{indent}{}=={}{suffix}{ending}",
                    plan.package, plan.target_version
                )
            })
            .collect::<String>();
        if replacements != 1 {
            return Err(AppError::Config(format!(
                "Expected one {} requirement, found {replacements}",
                plan.package
            )));
        }
        Ok(updated)
    }

    fn update_pyproject(content: &str, plan: &UpdatePlan) -> Result<String> {
        let mut document = content.parse::<DocumentMut>().map_err(|error| {
            AppError::Config(format!(
                "Cannot parse {}: {error}",
                plan.project_path.display()
            ))
        })?;
        let dependencies = document
            .get_mut("project")
            .and_then(|project| project.get_mut("dependencies"))
            .and_then(|dependencies| dependencies.as_array_mut())
            .ok_or_else(|| AppError::Config("No [project].dependencies array found".into()))?;
        let mut replacements = 0;
        for value in dependencies.iter_mut() {
            let matches = value
                .as_str()
                .map(|spec| {
                    normalize_package_name(requirement_name(spec))
                        == normalize_package_name(&plan.package)
                })
                .unwrap_or(false);
            if matches {
                Self::validate_python_spec(value.as_str().unwrap_or_default())?;
                let decor = value.decor().clone();
                *value =
                    toml_edit::Value::from(format!("{}=={}", plan.package, plan.target_version));
                *value.decor_mut() = decor;
                replacements += 1;
            }
        }
        if replacements != 1 {
            return Err(AppError::Config(format!(
                "Expected one {} dependency, found {replacements}",
                plan.package
            )));
        }
        Ok(document.to_string())
    }

    fn update_gemfile(content: &str, plan: &UpdatePlan) -> Result<String> {
        let mut replacements = 0;
        let mut validation_error = None;
        let updated = content
            .split_inclusive('\n')
            .map(|line| {
                let body = line.trim_end_matches(['\r', '\n']);
                let ending = &line[body.len()..];
                let (declaration, comment) = split_trailing_comment(body);
                let trimmed = declaration.trim_start();
                let Some(rest) = trimmed.strip_prefix("gem ") else {
                    return line.to_string();
                };
                let parts: Vec<&str> = rest.split(',').collect();
                let name = parts
                    .first()
                    .map(|part| part.trim().trim_matches('"').trim_matches('\''))
                    .unwrap_or_default();
                if name != plan.package {
                    return line.to_string();
                }
                if parts.len() > 2
                    && parts
                        .iter()
                        .skip(2)
                        .any(|part| part.chars().any(|character| character.is_ascii_digit()))
                {
                    validation_error = Some(AppError::Config(
                        "Refusing to rewrite compound Gem constraints; edit them manually".into(),
                    ));
                    return line.to_string();
                }
                replacements += 1;
                let indent = &declaration[..declaration.len() - trimmed.len()];
                let suffix = if comment.is_empty() {
                    String::new()
                } else {
                    format!(" {comment}")
                };
                if parts.len() < 2 {
                    return format!(
                        "{indent}gem \"{}\", \"= {}\"{suffix}{ending}",
                        plan.package, plan.target_version
                    );
                }
                let requirement = parts[1].trim();
                let quote = if requirement.contains('"') { '"' } else { '\'' };
                let unquoted = requirement.trim_matches('"').trim_matches('\'');
                let operator = if unquoted.starts_with("~>") || unquoted.starts_with(">=") {
                    let end = unquoted
                        .find(|character: char| character.is_ascii_digit())
                        .unwrap_or(unquoted.len());
                    &unquoted[..end]
                } else {
                    "= "
                };
                let options = parts
                    .get(2..)
                    .map(|parts| parts.join(","))
                    .unwrap_or_default();
                let options = if options.is_empty() {
                    options
                } else {
                    format!(",{}", options)
                };
                format!(
                    "{indent}gem \"{}\", {quote}{operator}{}{quote}{options}{suffix}{ending}",
                    plan.package, plan.target_version
                )
            })
            .collect::<String>();
        if let Some(error) = validation_error {
            return Err(error);
        }
        if replacements != 1 {
            return Err(AppError::Config(format!(
                "Expected one {} gem, found {replacements}",
                plan.package
            )));
        }
        Ok(updated)
    }

    fn validate_python_spec(specification: &str) -> Result<()> {
        if specification.contains('[')
            || specification.contains(';')
            || specification.contains(" @ ")
            || specification.contains(',')
            || specification.contains('<')
            || specification.contains("!=")
        {
            return Err(AppError::Config(
                "Refusing to rewrite a complex Python requirement; edit it manually".into(),
            ));
        }
        Ok(())
    }

    fn write_atomic(path: &Path, content: &str) -> Result<()> {
        let temporary = path.with_extension(format!("appchecker-{}.tmp", uuid::Uuid::new_v4()));
        let permissions = std::fs::metadata(path)?.permissions();
        std::fs::write(&temporary, content)?;
        std::fs::set_permissions(&temporary, permissions)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}

fn split_trailing_comment(line: &str) -> (&str, &str) {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '#' => return (&line[..index], &line[index..]),
            _ => {}
        }
    }
    (line, "")
}

fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group_killed = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !group_killed {
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

fn command(program: &str, args: Vec<String>) -> UpdateAction {
    UpdateAction::Command {
        program: program.into(),
        args,
    }
}

fn pyproject_declares_dependency(path: &Path, package: &str) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| content.parse::<toml::Value>().ok())
        .and_then(|document| {
            document
                .get("project")
                .and_then(|project| project.get("dependencies"))
                .and_then(|dependencies| dependencies.as_array())
                .map(|dependencies| {
                    dependencies.iter().any(|dependency| {
                        dependency
                            .as_str()
                            .map(|specification| {
                                normalize_package_name(requirement_name(specification))
                                    == normalize_package_name(package)
                                    && !crate::deps::pyproject_has_non_public_source(
                                        &document, package,
                                    )
                            })
                            .unwrap_or(false)
                    })
                })
        })
        .unwrap_or(false)
}

fn requirement_name(specification: &str) -> &str {
    let end = specification
        .find(|character: char| {
            character == '='
                || character == '>'
                || character == '<'
                || character == '!'
                || character == '~'
                || character == '['
                || character == ';'
                || character.is_whitespace()
        })
        .unwrap_or(specification.len());
    specification[..end].trim()
}

fn normalize_package_name(name: &str) -> String {
    let mut normalized = String::new();
    let mut separator = false;
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !separator {
                normalized.push('-');
            }
            separator = true;
        } else {
            normalized.extend(character.to_lowercase());
            separator = false;
        }
    }
    normalized
}
