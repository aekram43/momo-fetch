use std::path::{Path, PathBuf};

use crate::memory::parser;
use crate::memory::types::{
    Cluster, ConsolidationResult, ExtractionResult, ForesightValidation, MemoryQuery,
    MemoryResult, ProfileItem, ProfileOp, ProfileOpResult, Reflection, ReflectionPeriod,
    VaultConfig, VaultCounters, VaultStats,
};

/// Obsidian-compatible memory vault engine.
///
/// Manages all vault I/O with vault_path, config, and counters.
/// All methods are synchronous to avoid holding locks across await points.
pub struct ObsidianVault {
    vault_path: PathBuf,
    config: VaultConfig,
}

impl ObsidianVault {
    /// Open an existing vault or create a new one.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let config_path = path.join(".vault-config.json");
        let config: VaultConfig = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content)?
        } else {
            let mut config = VaultConfig::default();
            config.vault_path = path.to_string_lossy().to_string();
            config
        };

        // Ensure directory structure exists
        for dir in &[
            "1-memcells",
            "2-events",
            "3-foresights",
            "4-episodes",
            "5-profile",
            "6-reflections/weekly",
            "6-reflections/monthly",
            "clusters",
            "templates",
        ] {
            std::fs::create_dir_all(path.join(dir))?;
        }

        Ok(Self {
            vault_path: path.to_path_buf(),
            config,
        })
    }

    /// Get the vault root path.
    pub fn path(&self) -> &Path {
        &self.vault_path
    }

    /// Get the vault configuration.
    #[allow(dead_code)]
    pub fn config(&self) -> &VaultConfig {
        &self.config
    }

    /// Get vault statistics.
    pub fn stats(&self) -> &VaultStats {
        &self.config.stats
    }

    /// Get the vault counters.
    pub fn counters(&self) -> &VaultCounters {
        &self.config.counters
    }

    // ─── Write MemCell ───────────────────────────────────────────

    /// Write a MemCell to the daily log file.
    ///
    /// Appends a new MemCell section to `1-memcells/YYYY/MM/YYYY-MM-DD.md`.
    /// Creates the file with frontmatter if it doesn't exist.
    /// Updates config counters and regenerates index.
    pub fn write_memcell(
        &mut self,
        project: &str,
        topic: &str,
        context: &str,
        actions: &[crate::memory::types::ActionRecord],
        outcome: &str,
        keywords: &[&str],
    ) -> anyhow::Result<String> {
        let now = chrono::Local::now();
        let date = now.format("%Y-%m-%d").to_string();
        let time = now.format("%H:%M").to_string();

        let memcell_path = self.vault_path
            .join("1-memcells")
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(format!("{date}.md"));

        // Determine next MemCell number
        let memcell_num = self.count_memcells_in_file(&memcell_path)? + 1;
        let memcell_ref = format!("{date}#MemCell {memcell_num:03}");

        let actions_text = actions
            .iter()
            .enumerate()
            .map(|(i, a)| format!("{}. {} → {}", i + 1, a.description, a.result))
            .collect::<Vec<_>>()
            .join("\n");

        let keywords_str = keywords.join(", ");
        let tags_yaml = format!(
            "[memcell, {}]",
            keywords.iter()
                .chain(std::iter::once(&project))
                .map(|k| k.replace(' ', "-"))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let section = format!(

            "\n## MemCell {memcell_num:03} — {time}\n\n\
             **Topic**: {topic}\n\n\
             **Context**: {context}\n\n\
             **Actions**:\n{actions_text}\n\n\
             **Outcome**: {outcome}\n\n\
             **Keywords**: {keywords_str}\n",
        );

        // Ensure parent directories exist
        if let Some(parent) = memcell_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write or append
        if memcell_path.exists() {
            let existing = std::fs::read_to_string(&memcell_path)?;
            // Update memcell_count in frontmatter
            let updated = self.update_memcell_count(&existing, memcell_num);
            let tmp = atomic_temp_path(&memcell_path);
            std::fs::write(&tmp, format!("{updated}{section}"))?;
            std::fs::rename(&tmp, &memcell_path)?;
        } else {
            let frontmatter = format!(
                "---\n\
                 type: memcell\n\
                 date: {date}\n\
                 project: {project}\n\
                 tags: {tags_yaml}\n\
                 memcell_count: {memcell_num}\n\
                 ---\n",
            );
            let tmp = atomic_temp_path(&memcell_path);
            std::fs::write(&tmp, format!("{frontmatter}{section}"))?;
            std::fs::rename(&tmp, &memcell_path)?;
        }

        // Update counters
        self.config.stats.total_memcells += 1;
        self.save_config()?;
        self.regenerate_index()?;

        Ok(memcell_ref)
    }

    // ─── Extract ─────────────────────────────────────────────────

    /// Extract events, foresights, and episode from a MemCell.
    ///
    /// Creates structured notes from the raw MemCell data using templates.
    /// Updates counters and regenerates index.
    pub fn extract_from_memcell(
        &mut self,
        memcell_ref: &str,
        project: &str,
        topic: &str,
        context: &str,
        _actions: &[crate::memory::types::ActionRecord],
        outcome: &str,
        keywords: &[&str],
    ) -> anyhow::Result<ExtractionResult> {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut events_created = Vec::new();
        let mut foresights_created = Vec::new();

        // Extract events: one per significant keyword/topic
        let event_facts = self.generate_event_facts(topic, context, outcome, keywords);
        for fact in &event_facts {
            self.config.counters.event += 1;
            let event_id = format!("fact-{:04}", self.config.counters.event);
            let tags = format!("[fact, {}]",
                keywords.iter()
                    .take(3)
                    .map(|k| k.replace(' ', "-"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let content = format!(
                "---\n\
                 type: event\n\
                 id: {event_id}\n\
                 created: {timestamp}\n\
                 confidence: 0.85\n\
                 status: active\n\
                 parent: \"[[{memcell_ref}]]\"\n\
                 project: {project}\n\
                 tags: {tags}\n\
                 ---\n\n\
                 # {fact}\n\n\
                 {context}\n\n\
                 **Evidence**: {outcome}\n\n\
                 **Source**: [[{memcell_ref}]]\n",
            );

            let event_path = self.vault_path.join("2-events").join(format!("{event_id}.md"));
            let tmp = atomic_temp_path(&event_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &event_path)?;

            events_created.push(event_id);
            self.config.stats.total_events += 1;
        }

        // Extract foresight: one prediction based on patterns
        if let Some(prediction) = self.generate_foresight(topic, context, outcome, keywords) {
            self.config.counters.foresight += 1;
            let foresight_id = format!("pred-{:04}", self.config.counters.foresight);
            let duration_days = self.config.config.foresight_default_duration_days;
            let end_date = (now + chrono::Duration::days(duration_days as i64))
                .format("%Y-%m-%d")
                .to_string();

            let content = format!(
                "---\n\
                 type: foresight\n\
                 id: {foresight_id}\n\
                 created: {timestamp}\n\
                 status: pending\n\
                 confidence: 0.6\n\
                 start_time: {}\n\
                 end_time: {end_date}\n\
                 duration_days: {duration_days}\n\
                 parent: \"[[{memcell_ref}]]\"\n\
                 project: {project}\n\
                 tags: [foresight, {}]\n\
                 validation_criteria: \"{prediction}\"\n\
                 ---\n\n\
                 # {prediction}\n\n\
                 Based on current work on \"{topic}\", this prediction tracks whether the approach holds.\n\n\
                 **Evidence**: {outcome}\n\n\
                 **Validation**: On {end_date}, check if the prediction holds.\n",
                now.format("%Y-%m-%d"),
                keywords.first().unwrap_or(&"general").replace(' ', "-"),
            );

            let foresight_path = self.vault_path
                .join("3-foresights")
                .join(format!("{foresight_id}.md"));
            let tmp = atomic_temp_path(&foresight_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &foresight_path)?;

            foresights_created.push(foresight_id);
            self.config.stats.total_foresights += 1;
            self.config.stats.pending_foresights += 1;
        }

        // Generate/update daily episode
        let episode_id = self.generate_episode(
            &now.format("%Y-%m-%d").to_string(),
            project,
            topic,
            context,
            outcome,
            memcell_ref,
            &events_created,
            &foresights_created,
            keywords,
        )?;

        // Save config and regenerate index
        self.save_config()?;
        self.regenerate_index()?;

        Ok(ExtractionResult {
            memcell_ref: memcell_ref.to_string(),
            events_created,
            foresights_created,
            episode_id: Some(episode_id),
        })
    }

    // ─── Read Note ───────────────────────────────────────────────

    /// Read a note by its ID (e.g., "fact-0001", "pred-0001").
    pub fn read_note(&self, id: &str) -> anyhow::Result<Option<(PathBuf, String)>> {
        let path = if id.starts_with("fact-") {
            self.vault_path.join("2-events").join(format!("{id}.md"))
        } else if id.starts_with("pred-") {
            self.vault_path.join("3-foresights").join(format!("{id}.md"))
        } else if id.starts_with("ep-") {
            self.vault_path.join("4-episodes").join(format!("{id}.md"))
        } else if id.starts_with("cluster-") {
            self.vault_path.join("clusters").join(format!("{id}.md"))
        } else {
            return Ok(None);
        };

        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(Some((path, content)))
        } else {
            Ok(None)
        }
    }

    /// Search using specified retrieval mode.
    pub fn search(&self, query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
        match query.mode {
            crate::memory::types::RetrievalMode::GrepLlm => {
                crate::memory::retrieval::grep_llm(self, query)
            }
            crate::memory::types::RetrievalMode::GraphWalk => {
                crate::memory::retrieval::graph_walk(self, query)
            }
            crate::memory::types::RetrievalMode::TagFilter => {
                crate::memory::retrieval::tag_filter(self, query)
            }
            crate::memory::types::RetrievalMode::Agentic => {
                crate::memory::retrieval::agentic(self, query)
            }
        }
    }

    /// Get connected notes (outgoing wikilinks + backlinks) for a note.
    pub fn graph(&self, note_id: &str) -> anyhow::Result<Vec<MemoryResult>> {
        let mut results = Vec::new();

        // Outgoing links: parse wikilinks from the note content
        if let Some((_, content)) = self.read_note(note_id)? {
            let links = parser::extract_wikilinks(&content);
            for link in links {
                let clean_id = link.split('#').next().unwrap_or(&link).to_string();
                if let Some((path, link_content)) = self.read_note(&clean_id)? {
                    results.push(MemoryResult {
                        ref_id: clean_id,
                        level: crate::memory::retrieval::detect_level(&path),
                        relevance_score: 0.9,
                        snippet: crate::memory::retrieval::make_snippet(&link_content, &[]),
                        path,
                    });
                }
            }
        }

        // Backlinks: find all notes that reference this note
        let backlinks = crate::memory::retrieval::find_backlinks(self, note_id)?;
        results.extend(backlinks);

        // Deduplicate by ref_id
        let mut seen = std::collections::HashSet::new();
        results.retain(|r| seen.insert(r.ref_id.clone()));

        Ok(results)
    }

    /// Read the agent profile from the vault.
    pub fn read_profile(&self) -> anyhow::Result<Option<(PathBuf, String)>> {
        let profile_path = self.vault_path.join("5-profile").join("agent-profile.md");
        if profile_path.exists() {
            let content = std::fs::read_to_string(&profile_path)?;
            Ok(Some((profile_path, content)))
        } else {
            Ok(None)
        }
    }

    /// Read the user profile from the vault.
    pub fn read_user_profile(&self) -> anyhow::Result<Option<(PathBuf, String)>> {
        let profile_path = self.vault_path.join("5-profile").join("user-profile.md");
        if profile_path.exists() {
            let content = std::fs::read_to_string(&profile_path)?;
            Ok(Some((profile_path, content)))
        } else {
            Ok(None)
        }
    }

    // ─── Consolidation ───────────────────────────────────────────

    /// Consolidate memories into clusters and update agent profile.
    ///
    /// Scans MemCells for clusters (same project + overlapping keywords above
    /// threshold), creates cluster notes, and updates the agent profile with
    /// learned traits extracted from clusters.
    pub fn consolidate(&mut self) -> anyhow::Result<ConsolidationResult> {
        let mut clusters_created = Vec::new();
        let mut profile_ops = Vec::new();

        // 1. Collect all MemCell data (project, keywords, ref)
        let memcells = self.collect_memcell_data()?;

        if memcells.is_empty() {
            return Ok(ConsolidationResult {
                clusters_created,
                profile_ops,
                profile_compacted: false,
            });
        }

        // 2. Find clusters using Jaccard similarity on keywords
        let clusters = self.detect_clusters(&memcells);

        for cluster in clusters {
            self.config.counters.cluster += 1;
            let cluster_id = format!("cluster-{:03}", self.config.counters.cluster);
            let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();

            let memcell_links: Vec<String> = cluster
                .memcell_refs
                .iter()
                .map(|r| format!("[[{r}]]"))
                .collect();
            let tags = format!(
                "[cluster, {}]",
                cluster
                    .keywords
                    .iter()
                    .take(3)
                    .map(|k| k.replace(' ', "-"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let content = format!(
                "---\n\
                 type: cluster\n\
                 id: {cluster_id}\n\
                 created: {timestamp}\n\
                 project: {}\n\
                 topic: {}\n\
                 similarity: {:.2}\n\
                 memcell_count: {}\n\
                 tags: {tags}\n\
                 ---\n\n\
                 # Cluster: {}\n\n\
                 A grouping of {} related experiences.\n\n\
                 ## Keywords\n{}\n\n\
                 ## Member MemCells\n{}\n",
                cluster.project,
                cluster.topic,
                cluster.similarity,
                cluster.memcell_refs.len(),
                cluster.topic,
                cluster.memcell_refs.len(),
                cluster.keywords.join(", "),
                memcell_links.join("\n"),
            );

            let cluster_path = self
                .vault_path
                .join("clusters")
                .join(format!("{cluster_id}.md"));
            let tmp = atomic_temp_path(&cluster_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &cluster_path)?;

            clusters_created.push(cluster_id.clone());
            self.config.stats.total_clusters += 1;

            // 3. Derive profile items from cluster
            let item = ProfileItem {
                key: format!("{}_pattern", cluster.project),
                value: format!(
                    "Repeated work on '{}' across {} sessions. Keywords: {}",
                    cluster.topic,
                    cluster.memcell_refs.len(),
                    cluster.keywords.join(", ")
                ),
                confidence: cluster.similarity,
                source: format!("[[{cluster_id}]]"),
                updated: chrono::Local::now().format("%Y-%m-%d").to_string(),
            };

            let op_result = self.update_profile_item(ProfileOp::Add, item)?;
            profile_ops.push(op_result);
        }

        // 4. Profile compaction if needed
        let profile_compacted = self.maybe_compact_profile()?;

        self.save_config()?;
        self.regenerate_index()?;

        Ok(ConsolidationResult {
            clusters_created,
            profile_ops,
            profile_compacted,
        })
    }

    // ─── Foresight Validation ────────────────────────────────────

    /// Validate pending foresight predictions.
    ///
    /// Checks all foresights with status "pending" that have passed their
    /// end_time. Marks them as "expired" if the validation date has passed
    /// without confirmation.
    pub fn validate_foresights(&mut self) -> anyhow::Result<Vec<ForesightValidation>> {
        let mut validations = Vec::new();
        let foresights_dir = self.vault_path.join("3-foresights");
        if !foresights_dir.exists() {
            return Ok(validations);
        }

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        for entry in std::fs::read_dir(&foresights_dir)?.flatten() {
            if !entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "md")
            {
                continue;
            }

            let path = entry.path();
            let content = std::fs::read_to_string(&path)?;

            let (fm, _) = parser::parse_frontmatter(&content);
            let Some(fm) = fm else { continue };

            let status = fm
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if status != "pending" {
                continue;
            }

            let end_time = fm
                .get("end_time")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let foresight_id = fm
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if foresight_id.is_empty() {
                continue;
            }

            // If end_time has passed, mark as expired
            let new_status = if !end_time.is_empty() && end_time <= today {
                "expired"
            } else {
                continue; // Still within the prediction window
            };

            // Update the foresight file
            let updated = content.replace(
                &format!("status: {status}"),
                &format!("status: {new_status}"),
            );
            let tmp = atomic_temp_path(&path);
            std::fs::write(&tmp, &updated)?;
            std::fs::rename(&tmp, &path)?;

            if self.config.stats.pending_foresights > 0 {
                self.config.stats.pending_foresights -= 1;
            }

            validations.push(ForesightValidation {
                foresight_id,
                previous_status: status,
                new_status: new_status.to_string(),
                reason: format!("Prediction window ended on {end_time} (today: {today})"),
            });
        }

        if !validations.is_empty() {
            self.save_config()?;
        }

        Ok(validations)
    }

    // ─── Reflection ──────────────────────────────────────────────

    /// Generate a weekly or monthly reflection from recent memories.
    ///
    /// Summarizes MemCells, events, foresights, and clusters for the given
    /// period and writes the reflection to `6-reflections/<period>/`.
    pub fn reflect(&mut self, period: &ReflectionPeriod) -> anyhow::Result<Reflection> {
        let now = chrono::Local::now();
        let (days_back, period_dir) = match period {
            ReflectionPeriod::Weekly => (7, "6-reflections/weekly"),
            ReflectionPeriod::Monthly => (30, "6-reflections/monthly"),
        };

        let start_date = (now - chrono::Duration::days(days_back))
            .format("%Y-%m-%d")
            .to_string();
        let end_date = now.format("%Y-%m-%d").to_string();
        let date_range = format!("{start_date} to {end_date}");

        // Collect MemCells from the period
        let memcell_data = self.collect_memcell_data()?;
        let recent_memcells: Vec<_> = memcell_data
            .iter()
            .filter(|mc| mc.date >= start_date)
            .collect();

        let memcell_count = recent_memcells.len() as u32;

        // Extract themes from recent MemCells
        let themes = self.extract_themes(&recent_memcells);

        // Count foresights validated in this period
        let foresights_validated = self.count_foresights_in_period(&start_date, &end_date)?;

        // Build summary
        let summary = self.build_reflection_summary(
            &date_range,
            memcell_count,
            &themes,
            &recent_memcells,
        );

        // Create the reflection note
        self.config.counters.reflection += 1;
        let reflection_id = format!(
            "{}-{}-{:03}",
            period,
            now.format("%Y-%m-%d"),
            self.config.counters.reflection
        );

        let timestamp = now.format("%Y-%m-%dT%H:%M:%S").to_string();
        let tags = format!(
            "[reflection, {period}, {}]",
            themes
                .iter()
                .take(3)
                .map(|t| t.replace(' ', "-"))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let theme_list = themes
            .iter()
            .map(|t| format!("- {t}"))
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!(
            "---\n\
             type: reflection\n\
             id: {reflection_id}\n\
             period: {period}\n\
             created: {timestamp}\n\
             date_range: {date_range}\n\
             memcell_count: {memcell_count}\n\
             foresights_validated: {foresights_validated}\n\
             tags: {tags}\n\
             ---\n\n\
             # Reflection: {date_range}\n\n\
             ## Summary\n\n\
             {summary}\n\n\
             ## Key Themes\n\n\
             {theme_list}\n\n\
             ## Statistics\n\n\
             - MemCells analyzed: {memcell_count}\n\
             - Foresights validated: {foresights_validated}\n\
             - Period: {period}\n",
        );

        let refl_dir = self.vault_path.join(period_dir);
        std::fs::create_dir_all(&refl_dir)?;
        let refl_path = refl_dir.join(format!("{reflection_id}.md"));
        let tmp = atomic_temp_path(&refl_path);
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &refl_path)?;

        self.config.stats.total_reflections += 1;
        self.save_config()?;
        self.regenerate_index()?;

        Ok(Reflection {
            id: reflection_id,
            period: period.clone(),
            date_range,
            summary,
            themes,
            foresights_validated,
            memcell_count,
        })
    }

    // ─── Profile Management ──────────────────────────────────────

    /// Read the agent profile items from the profile file.
    pub fn read_profile_items(&self) -> anyhow::Result<Vec<ProfileItem>> {
        let profile_path = self.vault_path.join("5-profile").join("agent-profile.md");
        if !profile_path.exists() {
            return Ok(vec![]);
        }

        let content = std::fs::read_to_string(&profile_path)?;
        let (_, body) = parser::parse_frontmatter(&content);

        // Parse profile items from the body
        // Format: "**key**: value (confidence: 0.85, source: [[cluster-001]])"
        let mut items = Vec::new();
        for line in body.lines() {
            let line = line.trim();
            if let Some(item) = parse_profile_line(line) {
                items.push(item);
            }
        }

        Ok(items)
    }

    /// Write profile items to the agent profile file.
    fn write_profile_items(&mut self, items: &[ProfileItem]) -> anyhow::Result<()> {
        let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let item_count = items.len();

        let mut body = String::new();
        body.push_str("# Agent Profile\n\n");
        body.push_str("Learned traits and behavioral patterns.\n\n");

        for item in items {
            body.push_str(&format!(
                "**{}**: {} (confidence: {:.2}, source: {})\n",
                item.key, item.value, item.confidence, item.source
            ));
        }

        let content = format!(
            "---\n\
             type: profile\n\
             id: agent-profile\n\
             created: {timestamp}\n\
             updated: {timestamp}\n\
             item_count: {item_count}\n\
             ---\n\n\
             {body}",
        );

        let profile_path = self.vault_path.join("5-profile").join("agent-profile.md");
        let tmp = atomic_temp_path(&profile_path);
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &profile_path)?;

        self.config.stats.profile_items = items.len() as u64;
        Ok(())
    }

    /// Update a single profile item (ADD, UPDATE, or DELETE).
    fn update_profile_item(
        &mut self,
        operation: ProfileOp,
        new_item: ProfileItem,
    ) -> anyhow::Result<ProfileOpResult> {
        let mut items = self.read_profile_items()?;

        let key = new_item.key.clone();
        let existing_idx = items.iter().position(|i| i.key == new_item.key);

        match operation {
            ProfileOp::Add => {
                if existing_idx.is_some() {
                    // Key exists, treat as UPDATE instead
                    if let Some(idx) = existing_idx {
                        items[idx] = new_item.clone();
                    }
                } else {
                    items.push(new_item.clone());
                }
            }
            ProfileOp::Update => {
                if let Some(idx) = existing_idx {
                    items[idx] = new_item.clone();
                } else {
                    items.push(new_item.clone());
                }
            }
            ProfileOp::Delete => {
                items.retain(|i| i.key != new_item.key);
            }
        }

        self.write_profile_items(&items)?;

        Ok(ProfileOpResult {
            operation,
            key,
            value: Some(new_item.value),
            success: true,
        })
    }

    /// Compact the profile when items exceed the threshold.
    ///
    /// When profile items > profile_compact_threshold (default 37),
    /// consolidate to approximately profile_compact_ratio (default 0.7) of
    /// the threshold by keeping the highest-confidence items.
    fn maybe_compact_profile(&mut self) -> anyhow::Result<bool> {
        let items = self.read_profile_items()?;
        let threshold = self.config.config.profile_compact_threshold as usize;

        if items.len() <= threshold {
            return Ok(false);
        }

        // Sort by confidence descending, keep top items
        let mut sorted = items;
        sorted.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let target_count = ((threshold as f64) * self.config.config.profile_compact_ratio) as usize;
        let target_count = target_count.max(10).min(sorted.len());
        sorted.truncate(target_count);

        self.write_profile_items(&sorted)?;
        self.save_config()?;

        Ok(true)
    }

    // ─── Consolidation Helpers ───────────────────────────────────

    /// Collect MemCell data (project, keywords, date, ref) from vault files.
    fn collect_memcell_data(&self) -> anyhow::Result<Vec<MemCellData>> {
        let mut data = Vec::new();
        let memcells_dir = self.vault_path.join("1-memcells");
        if !memcells_dir.exists() {
            return Ok(data);
        }

        for entry in walkdir::WalkDir::new(&memcells_dir)
            .into_iter()
            .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path()
                        .extension()
                        .is_some_and(|ext: &std::ffi::OsStr| ext == "md")
            })
        {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let (fm, body) = parser::parse_frontmatter(&content);

                let project = fm
                    .as_ref()
                    .and_then(|v| v.get("project"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let date = fm
                    .as_ref()
                    .and_then(|v| v.get("date"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let file_stem = entry
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();

                // Extract keywords from **Keywords**: lines in the body
                let keywords = extract_keywords_from_body(body);

                // Count MemCells in this file (each ## MemCell section)
                let memcell_count = body
                    .lines()
                    .filter(|l| l.starts_with("## MemCell "))
                    .count();

                for i in 1..=memcell_count {
                    let memcell_ref = format!("{file_stem}#MemCell {i:03}");
                    data.push(MemCellData {
                        project: project.clone(),
                        date: date.clone(),
                        keywords: keywords.clone(),
                        memcell_ref,
                    });
                }
            }
        }

        Ok(data)
    }

    /// Detect clusters among MemCells using Jaccard similarity on keywords.
    fn detect_clusters(&self, memcells: &[MemCellData]) -> Vec<Cluster> {
        let threshold = self.config.config.cluster_similarity_threshold;
        let max_gap_days = self.config.config.cluster_max_time_gap_days as i64;
        let mut clusters: Vec<Cluster> = Vec::new();
        let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Group by project first
        let mut by_project: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, mc) in memcells.iter().enumerate() {
            by_project
                .entry(mc.project.clone())
                .or_default()
                .push(i);
        }

        for (_project, indices) in &by_project {
            for &i in indices {
                let mc_i = &memcells[i];
                if assigned.contains(&mc_i.memcell_ref) {
                    continue;
                }

                let mut cluster_refs = vec![mc_i.memcell_ref.clone()];
                let mut cluster_keywords = mc_i.keywords.clone();
                let cluster_date = mc_i.date.clone();

                for &j in indices {
                    if i == j {
                        continue;
                    }
                    let mc_j = &memcells[j];
                    if assigned.contains(&mc_j.memcell_ref) {
                        continue;
                    }

                    // Check time gap
                    let date_gap_ok = dates_within_days(&cluster_date, &mc_j.date, max_gap_days);
                    if !date_gap_ok {
                        continue;
                    }

                    // Compute Jaccard similarity
                    let similarity = jaccard_similarity(&cluster_keywords, &mc_j.keywords);
                    if similarity >= threshold {
                        cluster_refs.push(mc_j.memcell_ref.clone());
                        // Merge keywords
                        for kw in &mc_j.keywords {
                            if !cluster_keywords.contains(kw) {
                                cluster_keywords.push(kw.clone());
                            }
                        }
                        assigned.insert(mc_j.memcell_ref.clone());
                    }
                }

                if cluster_refs.len() >= self.config.config.consolidation_threshold as usize {
                    assigned.insert(mc_i.memcell_ref.clone());

                    // Pick the most common keyword as topic
                    let topic = cluster_keywords
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "general".to_string());

                    clusters.push(Cluster {
                        id: String::new(), // Assigned later
                        topic,
                        project: mc_i.project.clone(),
                        memcell_refs: cluster_refs,
                        keywords: cluster_keywords,
                        created: String::new(),
                        similarity: threshold,
                    });
                }
            }
        }

        clusters
    }

    /// Extract themes from a collection of MemCell data.
    fn extract_themes(&self, memcells: &[&MemCellData]) -> Vec<String> {
        let mut keyword_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for mc in memcells {
            for kw in &mc.keywords {
                *keyword_counts.entry(kw.clone()).or_insert(0) += 1;
            }
        }

        let mut themes: Vec<(String, usize)> = keyword_counts.into_iter().collect();
        themes.sort_by(|a, b| b.1.cmp(&a.1));

        themes.into_iter().take(5).map(|(t, _)| t).collect()
    }

    /// Count foresights that have been validated within a date range.
    fn count_foresights_in_period(
        &self,
        _start: &str,
        _end: &str,
    ) -> anyhow::Result<u32> {
        let foresights_dir = self.vault_path.join("3-foresights");
        if !foresights_dir.exists() {
            return Ok(0);
        }

        let mut count = 0u32;
        for entry in std::fs::read_dir(&foresights_dir)?.flatten() {
            if !entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "md")
            {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let (fm, _) = parser::parse_frontmatter(&content);
                let status = fm
                    .as_ref()
                    .and_then(|v| v.get("status"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // Count non-pending foresights (confirmed, disconfirmed, expired) as "validated"
                if status != "pending" && status != "" {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Build a text summary for a reflection.
    fn build_reflection_summary(
        &self,
        date_range: &str,
        memcell_count: u32,
        themes: &[String],
        memcells: &[&MemCellData],
    ) -> String {
        let project_counts = count_by_project(memcells);

        let mut summary = format!(
            "During {date_range}, {memcell_count} experiences were recorded across {} project(s).",
            project_counts.len()
        );

        if !themes.is_empty() {
            summary.push_str(&format!(
                "\n\nKey areas of focus: {}.",
                themes.join(", ")
            ));
        }

        if !project_counts.is_empty() {
            let proj_details: Vec<String> = project_counts
                .iter()
                .map(|(p, c)| format!("{p} ({c} sessions)"))
                .collect();
            summary.push_str(&format!(
                "\n\nProjects worked on: {}.",
                proj_details.join(", ")
            ));
        }

        summary
    }

    // ─── Config Persistence ──────────────────────────────────────

    /// Save the vault configuration to .vault-config.json.
    fn save_config(&self) -> anyhow::Result<()> {
        let config_path = self.vault_path.join(".vault-config.json");
        let json = serde_json::to_string_pretty(&self.config)?;
        let tmp = atomic_temp_path(&config_path);
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &config_path)?;
        Ok(())
    }

    // ─── Index Regeneration ──────────────────────────────────────

    /// Regenerate the vault index.md file.
    ///
    /// Walks all vault directories and builds a master table of contents.
    fn regenerate_index(&self) -> anyhow::Result<()> {
        let mut sections = Vec::new();

        // Header
        sections.push(format!(
            "---\n\
             auto-generated: true\n\
             last_updated: {}\n\
             ---\n\n\
             # Memory Vault\n\n\
             > EverMemOS-compatible memory hierarchy, stored as Obsidian wiki.\n\
             > Levels 1-6 map to raw experiences through abstract reflections.\n",
            chrono::Local::now().format("%Y-%m-%d"),
        ));

        // Level 1: MemCells
        sections.push(self.index_memcells());

        // Level 2: Events
        sections.push(self.index_events());

        // Level 3: Foresights
        sections.push(self.index_foresights());

        // Level 4: Episodes
        sections.push(self.index_episodes());

        // Level 5: Profiles
        sections.push(self.index_profiles());

        // Level 6: Reflections
        sections.push(self.index_reflections());

        // Clusters
        sections.push(self.index_clusters());

        let content = sections.join("\n");
        let index_path = self.vault_path.join("index.md");
        let tmp = atomic_temp_path(&index_path);
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &index_path)?;

        Ok(())
    }

    fn index_memcells(&self) -> String {
        let mut entries = Vec::new();
        let memcells_dir = self.vault_path.join("1-memcells");

        if memcells_dir.exists() {
            for entry in walkdir::WalkDir::new(&memcells_dir)
                .into_iter()
                .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
                .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext: &std::ffi::OsStr| ext == "md"))
            {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let (fm, _) = parser::parse_frontmatter(&content);
                    let date = fm
                        .as_ref()
                        .and_then(|v| v.get("date"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let count = fm
                        .as_ref()
                        .and_then(|v| v.get("memcell_count"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1);

                    let rel = entry
                        .path()
                        .file_stem()
                        .and_then(|s: &std::ffi::OsStr| s.to_str())
                        .unwrap_or(date);
                    entries.push(format!("- [[{rel}]] — {count} MemCell(s)"));
                }
            }
        }

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 1: MemCells (Raw Experiences)\n\n\
             > [!info] Daily logs of raw agent interactions\n\
             > Path: `1-memcells/YYYY/MM/YYYY-MM-DD.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_events(&self) -> String {
        let entries = self.index_level_dir("2-events", "fact-", "event");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 2: Events (Atomic Facts)\n\n\
             > [!info] One fact per note, linked back to source MemCell\n\
             > Path: `2-events/fact-NNNN.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_foresights(&self) -> String {
        let entries = self.index_level_dir("3-foresights", "pred-", "foresight");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 3: Foresights (Predictions)\n\n\
             > [!info] Time-bounded predictions with validation tracking\n\
             > Path: `3-foresights/pred-NNNN.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_episodes(&self) -> String {
        let entries = self.index_level_dir("4-episodes", "ep-", "episode");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 4: Episodes (Narrative Summaries)\n\n\
             > [!info] Clustered summaries of related MemCells\n\
             > Path: `4-episodes/ep-NNNN.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_profiles(&self) -> String {
        let profile_dir = self.vault_path.join("5-profile");
        let mut entries = Vec::new();

        if profile_dir.exists() {
            if let Ok(rd) = std::fs::read_dir(&profile_dir) {
                for entry in rd.flatten() {
                    if entry.path().extension().is_some_and(|ext| ext == "md") {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let stem = name.strip_suffix(".md").unwrap_or(&name);
                        entries.push(format!("- [[{stem}]] — Agent's own profile"));
                    }
                }
            }
        }

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 5: Profiles (Evolving Traits)\n\n\
             > [!info] Living profiles for agent and user\n\
             > Path: `5-profile/`\n\n\
             {entries_text}\n"
        )
    }

    fn index_reflections(&self) -> String {
        let mut entries = Vec::new();
        let reflections_dir = self.vault_path.join("6-reflections");

        if reflections_dir.exists() {
            for entry in walkdir::WalkDir::new(&reflections_dir)
                .into_iter()
                .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
                .filter(|e| {
                    e.file_type().is_file()
                        && e.path()
                            .extension()
                            .is_some_and(|ext: &std::ffi::OsStr| ext == "md")
                })
            {
                let name = entry.file_name().to_string_lossy().to_string();
                let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let title = extract_title(&content).unwrap_or_else(|| stem.clone());
                    entries.push(format!("- [[{stem}]] — {title}"));
                }
            }
        }

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 6: Reflections (Pattern Analysis)\n\n\
             > [!info] Weekly/monthly synthesis and decision weight adjustments\n\
             > Path: `6-reflections/`\n\n\
             {entries_text}\n"
        )
    }

    fn index_clusters(&self) -> String {
        let entries = self.index_level_dir("clusters", "cluster-", "cluster");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Clusters (Semantic Groupings)\n\n\
             > [!info] Virtual groupings of related MemCells for profile evolution\n\
             > Path: `clusters/`\n\n\
             {entries_text}\n"
        )
    }

    /// Index a level directory, extracting titles from frontmatter or first heading.
    fn index_level_dir(&self, dir: &str, prefix: &str, _level_name: &str) -> Vec<String> {
        let mut entries = Vec::new();
        let level_dir = self.vault_path.join(dir);

        if level_dir.exists() {
            if let Ok(rd) = std::fs::read_dir(&level_dir) {
                let mut files: Vec<_> = rd
                    .flatten()
                    .filter(|e| {
                        e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                            && e.path().extension().is_some_and(|ext| ext == "md")
                    })
                    .collect();
                files.sort_by_key(|e| e.file_name());

                for entry in files {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
                    if !stem.starts_with(prefix) {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        let title = extract_title(&content).unwrap_or_else(|| stem.clone());
                        let status = extract_status(&content);
                        let status_str = status
                            .map(|s| format!(" *({s})*"))
                            .unwrap_or_default();
                        entries.push(format!("- [[{stem}]] — {title}{status_str}"));
                    }
                }
            }
        }

        entries
    }

    // ─── Helpers ─────────────────────────────────────────────────

    /// Count MemCells in a daily log file.
    fn count_memcells_in_file(&self, path: &Path) -> anyhow::Result<u32> {
        if !path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(path)?;
        let count = content
            .lines()
            .filter(|line| line.starts_with("## MemCell "))
            .count();
        Ok(count as u32)
    }

    /// Update the memcell_count in the YAML frontmatter.
    fn update_memcell_count(&self, content: &str, new_count: u32) -> String {
        let re = regex::Regex::new(r"memcell_count:\s*\d+").unwrap();
        re.replace(content, format!("memcell_count: {new_count}"))
            .to_string()
    }

    /// Generate event facts from MemCell data.
    fn generate_event_facts(
        &self,
        topic: &str,
        context: &str,
        outcome: &str,
        keywords: &[&str],
    ) -> Vec<String> {
        let mut facts = Vec::new();

        // Primary fact from the topic
        facts.push(format!("{topic}: {outcome}"));

        // Secondary facts from keywords (limit to 3 to avoid noise)
        for kw in keywords.iter().take(3) {
            if !topic.to_lowercase().contains(&kw.to_lowercase()) {
                facts.push(format!(
                    "{kw}: {} related to {topic}",
                    if context.len() > 60 {
                        let end = context.ceil_char_boundary(60);
                        format!("{}...", &context[..end])
                    } else {
                        context.to_string()
                    }
                ));
            }
        }

        facts
    }

    /// Generate a foresight prediction from MemCell data.
    fn generate_foresight(
        &self,
        topic: &str,
        context: &str,
        outcome: &str,
        keywords: &[&str],
    ) -> Option<String> {
        // Only generate foresight if the MemCell suggests a pattern or decision
        let has_predictive_signal = keywords.iter().any(|k| {
            let k = k.to_lowercase();
            k.contains("pattern")
                || k.contains("trend")
                || k.contains("decision")
                || k.contains("strategy")
                || k.contains("approach")
                || k.contains("architecture")
        }) || context.to_lowercase().contains("will")
            || context.to_lowercase().contains("should")
            || context.to_lowercase().contains("plan");

        if has_predictive_signal {
            Some(format!(
                "Approach for \"{topic}\" will remain effective: {outcome}"
            ))
        } else {
            None
        }
    }

    /// Generate or update a daily episode note.
    fn generate_episode(
        &mut self,
        date: &str,
        project: &str,
        topic: &str,
        context: &str,
        outcome: &str,
        memcell_ref: &str,
        events: &[String],
        foresights: &[String],
        keywords: &[&str],
    ) -> anyhow::Result<String> {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S").to_string();

        // Check if there's already an episode for today's date
        let existing_episode = self.find_episode_for_date(date);

        let episode_id = if let Some((id, _content)) = &existing_episode {
            // Update existing episode with new MemCell reference
            let id = id.clone();
            self.append_to_episode(&id, memcell_ref, events, foresights, topic)?;
            id
        } else {
            // Create new episode
            self.config.counters.episode += 1;
            let id = format!("ep-{:04}", self.config.counters.episode);

            let events_links: Vec<String> = events.iter().map(|e| format!("[[{e}]]")).collect();
            let foresights_links: Vec<String> =
                foresights.iter().map(|f| format!("[[{f}]]")).collect();
            let tags = format!("[episode, {}]",
                keywords.iter()
                    .take(2)
                    .map(|k| k.replace(' ', "-"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let content = format!(
                "---\n\
                 type: episode\n\
                 id: {id}\n\
                 created: {timestamp}\n\
                 subject: {topic}\n\
                 source_memcells:\n\
                   - \"[[{memcell_ref}]]\"\n\
                 project: {project}\n\
                 tags: {tags}\n\
                 ---\n\n\
                 ## Summary\n\n\
                 {context}\n\n\
                 **Outcome**: {outcome}\n\n\
                 ## Related\n\
                 **Events**: {}\n\
                 **Foresights**: {}\n",
                events_links.join(", "),
                if foresights_links.is_empty() {
                    "(none)".to_string()
                } else {
                    foresights_links.join(", ")
                },
            );

            let ep_path = self.vault_path.join("4-episodes").join(format!("{id}.md"));
            let tmp = atomic_temp_path(&ep_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &ep_path)?;

            self.config.stats.total_episodes += 1;
            id
        };

        Ok(episode_id)
    }

    /// Find an episode that references the given date.
    fn find_episode_for_date(&self, date: &str) -> Option<(String, String)> {
        let episodes_dir = self.vault_path.join("4-episodes");
        if !episodes_dir.exists() {
            return None;
        }

        if let Ok(rd) = std::fs::read_dir(&episodes_dir) {
            for entry in rd.flatten() {
                if !entry.path().extension().is_some_and(|ext| ext == "md") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if content.contains(date) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let id = name.strip_suffix(".md").unwrap_or(&name).to_string();
                        return Some((id, content));
                    }
                }
            }
        }
        None
    }

    /// Append a new MemCell reference to an existing episode.
    fn append_to_episode(
        &self,
        episode_id: &str,
        memcell_ref: &str,
        events: &[String],
        foresights: &[String],
        topic: &str,
    ) -> anyhow::Result<()> {
        let ep_path = self.vault_path.join("4-episodes").join(format!("{episode_id}.md"));
        if !ep_path.exists() {
            return Ok(());
        }

        let mut content = std::fs::read_to_string(&ep_path)?;

        // Add new events and topic to the related section
        let new_events: Vec<String> = events.iter().map(|e| format!("[[{e}]]")).collect();
        let new_foresights: Vec<String> = foresights.iter().map(|f| format!("[[{f}]]")).collect();

        let addition = format!(
            "\n---\n\
             Additional from {memcell_ref}: {topic}\n\
             **New Events**: {}\n\
             **New Foresights**: {}\n",
            new_events.join(", "),
            if new_foresights.is_empty() {
                "(none)".to_string()
            } else {
                new_foresights.join(", ")
            },
        );

        content.push_str(&addition);

        let tmp = atomic_temp_path(&ep_path);
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &ep_path)?;

        Ok(())
    }
}

// ─── Utility Functions ───────────────────────────────────────────

/// Generate a temp file path for atomic writes.
fn atomic_temp_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(path);
    dir.join(format!("{stem}.harness-tmp{ext}"))
}

/// Extract the first markdown heading title from content.
fn extract_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return Some(trimmed[2..].to_string());
        }
    }
    None
}

/// Extract the status field from YAML frontmatter.
fn extract_status(content: &str) -> Option<String> {
    let (fm, _) = parser::parse_frontmatter(content);
    fm.and_then(|v| v.get("status")?.as_str().map(String::from))
}

// ─── Consolidation Helper Types ────────────────────────────────────

/// Intermediate data for a MemCell used during cluster detection.
struct MemCellData {
    project: String,
    date: String,
    keywords: Vec<String>,
    memcell_ref: String,
}

/// Extract keywords from the body of a MemCell file.
fn extract_keywords_from_body(body: &str) -> Vec<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("**Keywords**:") {
            return rest
                .trim()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    vec![]
}

/// Compute Jaccard similarity between two keyword sets.
fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }

    let set_a: std::collections::HashSet<String> =
        a.iter().map(|s| s.to_lowercase()).collect();
    let set_b: std::collections::HashSet<String> =
        b.iter().map(|s| s.to_lowercase()).collect();

    let intersection = set_a.intersection(&set_b).count() as f64;
    let union = set_a.union(&set_b).count() as f64;

    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Check if two date strings (YYYY-MM-DD) are within a given number of days.
fn dates_within_days(date1: &str, date2: &str, max_days: i64) -> bool {
    let d1 = chrono::NaiveDate::parse_from_str(date1, "%Y-%m-%d");
    let d2 = chrono::NaiveDate::parse_from_str(date2, "%Y-%m-%d");

    match (d1, d2) {
        (Ok(d1), Ok(d2)) => {
            let diff = (d1 - d2).num_days().abs();
            diff <= max_days
        }
        _ => true, // If dates can't be parsed, don't filter out
    }
}

/// Parse a profile item line from the profile file.
fn parse_profile_line(line: &str) -> Option<ProfileItem> {
    // Format: "**key**: value (confidence: 0.85, source: [[cluster-001]])"
    if !line.starts_with("**") {
        return None;
    }

    // Extract key between ** markers
    let rest_after_open = line.get(2..)?;
    let close_idx = rest_after_open.find("**")?;
    let key = rest_after_open[..close_idx].to_string();
    let rest = rest_after_open.get(close_idx + 2..)?.trim();

    // Split value from metadata
    let value_end = rest.rfind("(confidence:")?;
    let value = rest.get(..value_end)?.trim().trim_start_matches(':').trim().to_string();

    // Extract confidence
    let conf_start = rest.find("confidence:")?;
    let conf_str = &rest[conf_start + 11..];
    let conf_end = conf_str.find(',')?;
    let confidence: f64 = conf_str[..conf_end].trim().parse().ok()?;

    // Extract source
    let src_start = rest.find("source:")?;
    let src_str = &rest[src_start + 7..];
    let src_end = src_str.find(')')?;
    let source = src_str[..src_end].trim().to_string();

    Some(ProfileItem {
        key,
        value,
        confidence,
        source,
        updated: chrono::Local::now().format("%Y-%m-%d").to_string(),
    })
}

/// Count MemCells by project.
fn count_by_project(memcells: &[&MemCellData]) -> Vec<(String, u32)> {
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for mc in memcells {
        *counts.entry(mc.project.clone()).or_insert(0) += 1;
    }
    let mut result: Vec<(String, u32)> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::ActionRecord;

    #[test]
    fn test_vault_open_create() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        assert!(vault.path().exists());
        assert!(tmp.path().join("1-memcells").exists());
        assert!(tmp.path().join("2-events").exists());
        assert!(tmp.path().join("3-foresights").exists());
        assert!(tmp.path().join("4-episodes").exists());
        assert!(tmp.path().join("5-profile").exists());
        assert!(tmp.path().join("6-reflections/weekly").exists());
        assert!(tmp.path().join("6-reflections/monthly").exists());
        assert!(tmp.path().join("clusters").exists());
        assert!(tmp.path().join("templates").exists());
    }

    #[test]
    fn test_vault_open_existing_config() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a pre-existing config
        let config = VaultConfig::default();
        let config_path = tmp.path().join(".vault-config.json");
        std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

        let vault = ObsidianVault::open(tmp.path()).unwrap();
        assert_eq!(vault.config().schema_version, "1.0.0");
    }

    #[test]
    fn test_write_memcell_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let memcell_ref = vault
            .write_memcell(
                "test-project",
                "Test Topic",
                "Testing vault write",
                &[ActionRecord {
                    description: "wrote test".into(),
                    result: "success".into(),
                }],
                "Works correctly",
                &["test", "vault"],
            )
            .unwrap();

        assert!(memcell_ref.contains("MemCell 001"));
        assert_eq!(vault.stats().total_memcells, 1);

        // Verify config was saved
        let config_path = tmp.path().join(".vault-config.json");
        assert!(config_path.exists());
        let saved: VaultConfig =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(saved.stats.total_memcells, 1);

        // Verify index was regenerated
        let index_path = tmp.path().join("index.md");
        assert!(index_path.exists());
    }

    #[test]
    fn test_write_memcell_multiple_same_day() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let ref1 = vault
            .write_memcell(
                "test-project",
                "First",
                "ctx1",
                &[],
                "ok1",
                &["test"],
            )
            .unwrap();

        let ref2 = vault
            .write_memcell(
                "test-project",
                "Second",
                "ctx2",
                &[],
                "ok2",
                &["test"],
            )
            .unwrap();

        assert!(ref1.contains("MemCell 001"));
        assert!(ref2.contains("MemCell 002"));
        assert_eq!(vault.stats().total_memcells, 2);
    }

    #[test]
    fn test_extract_from_memcell() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let result = vault
            .extract_from_memcell(
                "2026-05-07#MemCell 001",
                "test-project",
                "Architecture Decision",
                "Chose Obsidian wiki for vault",
                &[ActionRecord {
                    description: "researched options".into(),
                    result: "found good match".into(),
                }],
                "Obsidian wiki selected as vault backend",
                &["architecture", "decision", "vault"],
            )
            .unwrap();

        // Should have created events (one for topic + up to 2 keyword facts)
        assert!(!result.events_created.is_empty());
        assert_eq!(vault.stats().total_events, result.events_created.len() as u64);

        // Should have created a foresight (has "decision" keyword)
        assert!(!result.foresights_created.is_empty());
        assert_eq!(
            vault.stats().total_foresights,
            result.foresights_created.len() as u64
        );

        // Should have created an episode
        assert!(result.episode_id.is_some());
        assert_eq!(vault.stats().total_episodes, 1);

        // Verify event file exists
        let event_id = &result.events_created[0];
        let event_path = tmp.path().join("2-events").join(format!("{event_id}.md"));
        assert!(event_path.exists());

        // Verify foresight file exists
        let foresight_id = &result.foresights_created[0];
        let foresight_path = tmp
            .path()
            .join("3-foresights")
            .join(format!("{foresight_id}.md"));
        assert!(foresight_path.exists());

        // Verify episode file exists
        let episode_id = result.episode_id.as_ref().unwrap();
        let episode_path = tmp
            .path()
            .join("4-episodes")
            .join(format!("{episode_id}.md"));
        assert!(episode_path.exists());
    }

    #[test]
    fn test_read_note() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Create a note via extraction
        let result = vault
            .extract_from_memcell(
                "2026-05-07#MemCell 001",
                "test-project",
                "Test Topic",
                "test context",
                &[],
                "test outcome",
                &["test"],
            )
            .unwrap();

        let event_id = &result.events_created[0];
        let note = vault.read_note(event_id).unwrap();
        assert!(note.is_some());
        let (_, content) = note.unwrap();
        assert!(content.contains("type: event"));
        assert!(content.contains(event_id));

        // Non-existent note
        let missing = vault.read_note("fact-9999").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_extract_no_foresight_without_predictive_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let result = vault
            .extract_from_memcell(
                "2026-05-07#MemCell 001",
                "test-project",
                "Fixed typo",
                "Fixed a typo in README",
                &[],
                "Typo fixed",
                &["typo", "readme"],
            )
            .unwrap();

        // Should have events but NO foresight (no predictive keywords)
        assert!(!result.events_created.is_empty());
        assert!(result.foresights_created.is_empty());
    }

    #[test]
    fn test_write_then_extract_full_flow() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Write a MemCell
        let memcell_ref = vault
            .write_memcell(
                "momo-fetch",
                "Memory vault architecture",
                "Designing the Obsidian-based memory system",
                &[
                    ActionRecord {
                        description: "analyzed EverMemOS paper".into(),
                        result: "identified 6 levels".into(),
                    },
                    ActionRecord {
                        description: "designed directory structure".into(),
                        result: "complete".into(),
                    },
                ],
                "Architecture approved and ready for implementation",
                &["memory", "architecture", "obsidian"],
            )
            .unwrap();

        // Extract from it
        let result = vault
            .extract_from_memcell(
                &memcell_ref,
                "momo-fetch",
                "Memory vault architecture",
                "Designing the Obsidian-based memory system",
                &[
                    ActionRecord {
                        description: "analyzed EverMemOS paper".into(),
                        result: "identified 6 levels".into(),
                    },
                    ActionRecord {
                        description: "designed directory structure".into(),
                        result: "complete".into(),
                    },
                ],
                "Architecture approved and ready for implementation",
                &["memory", "architecture", "obsidian"],
            )
            .unwrap();

        // Verify full chain
        assert_eq!(vault.stats().total_memcells, 1);
        assert!(vault.stats().total_events >= 1);
        assert!(vault.stats().total_foresights >= 1); // "architecture" is predictive
        assert_eq!(vault.stats().total_episodes, 1);
    }

    #[test]
    fn test_config_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();

        // Create vault with default config
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Modify and save
        vault.config.counters.event = 42;
        vault.config.stats.total_memcells = 100;
        vault.save_config().unwrap();

        // Re-open and verify
        let vault2 = ObsidianVault::open(tmp.path()).unwrap();
        assert_eq!(vault2.config.counters.event, 42);
        assert_eq!(vault2.config.stats.total_memcells, 100);
    }

    #[test]
    fn test_consolidate_empty_vault() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let result = vault.consolidate().unwrap();
        assert!(result.clusters_created.is_empty());
        assert!(result.profile_ops.is_empty());
        assert!(!result.profile_compacted);
    }

    #[test]
    fn test_consolidate_with_data() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Write several MemCells with overlapping keywords on the same project
        for i in 0..6 {
            vault
                .write_memcell(
                    "test-project",
                    &format!("Rust async patterns {}", i),
                    &format!("Working on async code {}", i),
                    &[ActionRecord {
                        description: "coded".into(),
                        result: "works".into(),
                    }],
                    "Done",
                    &["rust", "async", "tokio"],
                )
                .unwrap();
        }

        let result = vault.consolidate().unwrap();

        // Should have created at least one cluster (6 MemCells, same project, same keywords)
        assert!(!result.clusters_created.is_empty());
        assert!(vault.stats().total_clusters > 0);

        // Verify cluster file exists
        let cluster_id = &result.clusters_created[0];
        let cluster_path = tmp.path().join("clusters").join(format!("{cluster_id}.md"));
        assert!(cluster_path.exists());
        let content = std::fs::read_to_string(&cluster_path).unwrap();
        assert!(content.contains("type: cluster"));
        assert!(content.contains(cluster_id));

        // Profile should have been updated
        assert!(vault.stats().profile_items > 0);
    }

    #[test]
    fn test_validate_foresights_marks_expired() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Create a foresight that has already expired
        vault.config.counters.foresight = 1;
        let past_date = "2020-01-01".to_string();
        let content = format!(
            "---\n\
             type: foresight\n\
             id: pred-0001\n\
             status: pending\n\
             end_time: {past_date}\n\
             ---\n\n\
             # Test prediction\n",
        );
        let foresight_path = tmp.path().join("3-foresights").join("pred-0001.md");
        let tmp_path = super::atomic_temp_path(&foresight_path);
        std::fs::write(&tmp_path, &content).unwrap();
        std::fs::rename(&tmp_path, &foresight_path).unwrap();
        vault.config.stats.pending_foresights = 1;
        vault.save_config().unwrap();

        let validations = vault.validate_foresights().unwrap();
        assert_eq!(validations.len(), 1);
        assert_eq!(validations[0].foresight_id, "pred-0001");
        assert_eq!(validations[0].previous_status, "pending");
        assert_eq!(validations[0].new_status, "expired");
        assert_eq!(vault.stats().pending_foresights, 0);

        // Verify file was updated
        let updated = std::fs::read_to_string(&foresight_path).unwrap();
        assert!(updated.contains("status: expired"));
    }

    #[test]
    fn test_validate_foresights_skips_active() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Create a foresight with future end_time
        vault.config.counters.foresight = 1;
        let future_date = "2099-12-31".to_string();
        let content = format!(
            "---\n\
             type: foresight\n\
             id: pred-0001\n\
             status: pending\n\
             end_time: {future_date}\n\
             ---\n\n\
             # Future prediction\n",
        );
        let foresight_path = tmp.path().join("3-foresights").join("pred-0001.md");
        let tmp_path = super::atomic_temp_path(&foresight_path);
        std::fs::write(&tmp_path, &content).unwrap();
        std::fs::rename(&tmp_path, &foresight_path).unwrap();
        vault.config.stats.pending_foresights = 1;
        vault.save_config().unwrap();

        let validations = vault.validate_foresights().unwrap();
        assert!(validations.is_empty());
        assert_eq!(vault.stats().pending_foresights, 1);
    }

    #[test]
    fn test_reflect_weekly() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Write some MemCells
        vault
            .write_memcell(
                "test-project",
                "Test Topic",
                "Test context",
                &[],
                "Done",
                &["test", "reflection"],
            )
            .unwrap();

        let result = vault
            .reflect(&crate::memory::types::ReflectionPeriod::Weekly)
            .unwrap();

        assert!(result.id.starts_with("weekly-"));
        assert!(!result.themes.is_empty());
        assert_eq!(result.memcell_count, 1);
        assert_eq!(vault.stats().total_reflections, 1);

        // Verify reflection file was created
        let refl_dir = tmp.path().join("6-reflections").join("weekly");
        assert!(refl_dir.exists());
        let refl_files: Vec<_> = std::fs::read_dir(&refl_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "md")
            })
            .collect();
        assert_eq!(refl_files.len(), 1);
    }

    #[test]
    fn test_reflect_monthly() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let result = vault
            .reflect(&crate::memory::types::ReflectionPeriod::Monthly)
            .unwrap();

        assert!(result.id.starts_with("monthly-"));
        assert_eq!(vault.stats().total_reflections, 1);
    }

    #[test]
    fn test_profile_management() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Add a profile item
        let item = crate::memory::types::ProfileItem {
            key: "test_preference".into(),
            value: "prefers concise responses".into(),
            confidence: 0.9,
            source: "[[cluster-001]]".into(),
            updated: "2026-05-07".into(),
        };

        let op_result = vault
            .update_profile_item(crate::memory::types::ProfileOp::Add, item)
            .unwrap();
        assert!(op_result.success);

        // Read back
        let items = vault.read_profile_items().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "test_preference");
        assert_eq!(items[0].confidence, 0.9);

        // Update
        let updated_item = crate::memory::types::ProfileItem {
            key: "test_preference".into(),
            value: "prefers detailed responses".into(),
            confidence: 0.95,
            source: "[[cluster-002]]".into(),
            updated: "2026-05-08".into(),
        };
        vault
            .update_profile_item(crate::memory::types::ProfileOp::Update, updated_item)
            .unwrap();

        let items = vault.read_profile_items().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].value, "prefers detailed responses");

        // Delete
        let delete_item = crate::memory::types::ProfileItem {
            key: "test_preference".into(),
            value: String::new(),
            confidence: 0.0,
            source: String::new(),
            updated: String::new(),
        };
        vault
            .update_profile_item(crate::memory::types::ProfileOp::Delete, delete_item)
            .unwrap();

        let items = vault.read_profile_items().unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_jaccard_similarity() {
        let a = vec!["rust".into(), "async".into(), "tokio".into()];
        let b = vec!["rust".into(), "async".into(), "tokio".into()];
        assert_eq!(super::jaccard_similarity(&a, &b), 1.0);

        let c = vec!["python".into(), "django".into()];
        assert_eq!(super::jaccard_similarity(&a, &c), 0.0);

        let d = vec!["rust".into(), "database".into()];
        let sim = super::jaccard_similarity(&a, &d);
        assert!(sim > 0.0 && sim < 1.0); // 1 overlap out of 4 unique = 0.25
    }

    #[test]
    fn test_stats_include_new_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        let stats = vault.stats();
        assert_eq!(stats.total_clusters, 0);
        assert_eq!(stats.total_reflections, 0);
        assert_eq!(stats.profile_items, 0);
    }
}
