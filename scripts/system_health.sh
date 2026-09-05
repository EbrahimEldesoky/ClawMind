#!/usr/bin/env bash
# Description: Gathers comprehensive hardware diagnostics (CPU, RAM, Disk, Load Average, Top Processes)
# Usage: system_health.sh [--json]

set -e

JSON_MODE=false
if [ "$1" == "--json" ]; then
    JSON_MODE=true
fi

CPU_MODEL=$(lscpu | grep "Model name:" | sed 's/Model name:[ \t]*//' | head -n1 || uname -p)
CORES=$(nproc || echo 1)
LOAD_AVG=$(uptime | awk -F'load average:' '{ print $2 }' | sed 's/^[ \t]*//')

MEM_TOTAL=$(free -m | awk '/^Mem:/{print $2}')
MEM_USED=$(free -m | awk '/^Mem:/{print $3}')
MEM_PCT=$(( 100 * MEM_USED / (MEM_TOTAL > 0 ? MEM_TOTAL : 1) ))

DISK_INFO=$(df -h / | awk 'NR==2 {print $2, $3, $4, $5}')
DISK_TOTAL=$(echo "$DISK_INFO" | awk '{print $1}')
DISK_USED=$(echo "$DISK_INFO" | awk '{print $2}')
DISK_AVAIL=$(echo "$DISK_INFO" | awk '{print $3}')
DISK_PCT=$(echo "$DISK_INFO" | awk '{print $4}')

if [ "$JSON_MODE" = true ]; then
    cat <<EOF
{
  "cpu": {
    "model": "$CPU_MODEL",
    "cores": $CORES,
    "load_average": "$LOAD_AVG"
  },
  "memory": {
    "total_mb": $MEM_TOTAL,
    "used_mb": $MEM_USED,
    "usage_pct": $MEM_PCT
  },
  "disk": {
    "root_total": "$DISK_TOTAL",
    "root_used": "$DISK_USED",
    "root_avail": "$DISK_AVAIL",
    "usage_pct": "$DISK_PCT"
  }
}
EOF
else
    echo "=================================================================="
    echo " 🖥️  ClawMind System Health Diagnostic Report"
    echo "=================================================================="
    echo " • CPU Model      : $CPU_MODEL"
    echo " • Cores Available: $CORES"
    echo " • Load Average   : $LOAD_AVG"
    echo " • Memory Usage   : ${MEM_USED}MB / ${MEM_TOTAL}MB (${MEM_PCT}%)"
    echo " • Root Disk Usage: ${DISK_USED} / ${DISK_TOTAL} (${DISK_PCT}) - Free: ${DISK_AVAIL}"
    echo "------------------------------------------------------------------"
    echo " 🔝 Top 3 Memory Consuming Processes:"
    ps -eo pid,ppid,cmd,%mem,%cpu --sort=-%mem | head -n 4 | awk '{printf "   PID: %-7s CPU: %-5s MEM: %-5s CMD: %s\n", $1, $5"%", $4"%", $3}'
    echo "=================================================================="
fi
