---
date: <% tp.date.now("YYYY-MM-DD") %>
type: <% tp.system.prompt("Note Type (semantic-decision / failed-experiment / edge-case / pattern / session-lifecycle / best-practice / harness-idiom)") %>
tags: [<% tp.system.prompt("Main tag") %>]
harness: <% tp.system.prompt("harness (korg / cli-anything / api-anything / redmicro / other)") %>
domain: <% tp.system.prompt("domain (orchestration / acp / session-model / supervision / artifact-emission / recovery / tui / etc.)") %>
status: active
---

## For future Grok

<% tp.system.prompt("2-4 sentence summary of what this note is about and why it matters") %>

---

# <% tp.file.title %>

## Context

## Details

## Links & Related

## Notes
