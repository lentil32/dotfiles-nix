# Mermaid Preview

Quick check for Snacks image previews in markdown.

```mermaid
flowchart LR
  A[Open file] --> B{Preview?}
  B -- "Mermaid OK" --> C[Render diagram]
  B -- "Missing deps" --> D[Show text]
  D --> B
```
