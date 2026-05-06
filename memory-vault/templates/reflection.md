---
type: reflection
period: {{period}}
{{#week}}week: {{week}}{{/week}}
{{#month}}month: {{month}}{{/month}}
date_range: [{{start_date}}, {{end_date}}]
episode_count: {{episode_count}}
tags: [reflection, {{period}}, patterns]
---

# {{period_title}}

## Patterns Identified

{{#patterns}}
### {{ordinal}}. {{pattern_title}}
{{pattern_description}}
**Episodes**: {{#evidence}}[[{{ep}}]], {{/evidence}}
{{/patterns}}

## Decision Weight Adjustments

| Decision | Old Weight | New Weight | Reason |
|----------|-----------|-----------|--------|
{{#weights}}| {{decision}} | {{old}} | {{new}} | {{reason}} |
{{/weights}}

## Foresight Validation

| Foresight | Status | Notes |
|-----------|--------|-------|
{{#foresights}}| [[{{foresight_id}}]] {{title}} | {{status}} | {{notes}} |
{{/foresights}}
