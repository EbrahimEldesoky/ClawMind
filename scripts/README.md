# ClawMind Client Automation Scripts Repository 

Welcome to your local automation repository!
Any script placed in this folder is automatically indexed, understood, and executable by **ClawMind** via **Script-RAG**.

---

##  Supported Script Types
ClawMind can automatically detect and run:
- **Bash / Shell Scripts**: `*.sh`, `*.bash` (invoked via `bash`)
- **Python Scripts**: `*.py` (invoked via `python3`)
- **Node.js Scripts**: `*.js`, `*.mjs` (invoked via `node`)
- **Native Binaries / Executables**: any compiled executable or binary

---

##  How to Document Your Scripts for ClawMind
ClawMind's **Script-RAG** reads the header and comments of your scripts so the agent instantly knows **what the script does** and **what parameters it requires**.

### Example 1: Bash Script (`example.sh`)
```bash
#!/bin/bash
# Description: Generates daily system diagnostic metrics and disk usage report
# Usage: ./system_health.sh [--detailed]

if [ "$1" == "--detailed" ]; then
    echo "Running detailed diagnostics..."
else
    echo "Running standard diagnostics..."
fi
```

### Example 2: Python Script (`backup_database.py`)
```python
#!/usr/bin/env python3
"""
Description: Connects to local or remote database and creates a timestamped SQL dump.
Usage: backup_database.py --db <db_name> [--output-dir /backups]
"""
import sys

# Your automation code here...
```

---

##  Interacting with ClawMind
You can ask ClawMind naturally in Arabic or English to run, inspect, or manage any script:
- *"شغّل اسكريبت فحص صحة النظام"*
- *"Run the database backup script for mydb"*
- *"اعرض لي الاسكريبتات المتاحة"* (or type `/scripts` in the terminal)
- *"Inspect how the report generator works"*
- *"اكتب اسكريبت بايثون جديد في scripts لمسح الملفات المؤقتة"*
