---
type: episode
id: ep-{{id}}
created: {{created}}
subject: {{subject}}
cluster: "[[{{cluster}}]]"
source_memcells:
{{#memcells}}  - "[[{{memcell}}]]"
{{/memcells}}
project: {{project}}
tags: [episode{{#tags}}, {{tag}}{{/tags}}]
---

## Summary

{{summary}}

## Key Decisions
{{#decisions}}- {{decision}}
{{/decisions}}

## Outcomes
{{#outcomes}}- {{outcome}}
{{/outcomes}}

## Lessons
{{#lessons}}- {{lesson}}
{{/lessons}}

## Related
**Events**: {{#events}}[[{{event}}]], {{/events}}
**Foresights**: {{#foresights}}[[{{foresight}}]], {{/foresights}}
**Reflection**: {{#reflections}}[[{{reflection}}]], {{/reflections}}
