# Korg - Queries

This note contains useful Dataview queries for exploring the Korg vault as an Operational Intelligence Layer.

---

## All Semantic Decisions

```dataview
TABLE date, harness, domain, status
FROM "wiki"
WHERE type = "semantic-decision"
SORT date DESC
```

## All Patterns (by domain)

```dataview
TABLE date, harness, domain, status
FROM "wiki"
WHERE type = "pattern"
SORT domain ASC
```

## Failed Experiments + Edge Cases

```dataview
TABLE date, type, harness, domain
FROM "wiki"
WHERE type = "failed-experiment" OR type = "edge-case"
SORT date DESC
```

## Recent High-Value Notes

```dataview
TABLE date, type, harness
FROM "wiki"
WHERE type != "daily"
SORT date DESC
LIMIT 15
```

## Notes by Harness

```dataview
TABLE date, type, domain
FROM "wiki"
WHERE harness = "korg"
SORT date DESC
```

*(Change `"korg"` to any other harness name as needed)*

## Notes by Domain

```dataview
TABLE date, type, harness
FROM "wiki"
WHERE domain = "orchestration"
SORT date DESC
```

*(Change domain value as needed)*
