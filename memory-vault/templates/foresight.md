---
type: foresight
id: pred-{{id}}
created: {{created}}
status: pending
confidence: {{confidence}}
start_time: {{start_time}}
end_time: {{end_time}}
duration_days: {{duration_days}}
parent: "[[{{parent}}]]"
project: {{project}}
tags: [foresight{{#tags}}, {{tag}}{{/tags}}]
validation_criteria: "{{validation_criteria}}"
---

# {{title}}

{{body}}

**Evidence**: {{evidence}}

**Validation**: On {{end_time}}, check if:
- {{#criteria}}{{criterion}}
- {{/criteria}}
