#!/usr/bin/env python3
"""Consistent online SQLite backup. Never copy a live WAL database file alone."""
import os
from pathlib import Path
import sqlite3
import sys


def main():
    if len(sys.argv) != 3:
        raise SystemExit('Usage: python3 scripts/backup.py data/pokerdraft.db /backup/pokerdraft.db')
    source, destination = map(lambda s: Path(s).resolve(), sys.argv[1:])
    if not source.is_file() or source == destination or destination.exists():
        raise SystemExit('Source must exist; destination must be a new, different file.')
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(destination, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(descriptor)
    with sqlite3.connect(source.as_uri() + '?mode=ro', uri=True) as db:
        with sqlite3.connect(destination) as backup:
            db.backup(backup, pages=256)
            assert backup.execute('PRAGMA integrity_check').fetchone()[0] == 'ok'
    print(f'Verified backup: {destination}')


if __name__ == '__main__':
    main()
