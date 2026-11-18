#!/usr/bin/env python3
"""Rehearse restoring an Analiz rabot SQLite backup into a NEW file.

Uses SQLite Online Backup (including WAL). Does not change the source or an
existing destination. This is a verification tool, not a live-database swap.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile


def _read_only(path: Path) -> sqlite3.Connection:
    return sqlite3.connect(path.as_uri() + '?mode=ro', uri=True, timeout=10)


def _healthy(conn: sqlite3.Connection) -> None:
    rows = conn.execute('PRAGMA quick_check').fetchall()
    if rows != [('ok',)]:
        raise ValueError('SQLite integrity check failed: ' + repr(rows[:4]))
    names = {name for (name,) in conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('reports','kv')"
    )}
    if names != {'reports', 'kv'}:
        raise ValueError('Not an Analiz rabot database: reports/kv tables are required')
    # Validate one report at a time, never the entire report history in memory.
    for (raw,) in conn.execute('SELECT json FROM reports'):
        if not isinstance(json.loads(raw), dict):
            raise ValueError('Invalid report JSON object')
    for (raw,) in conn.execute('SELECT json FROM kv'):
        json.loads(raw)


def _digest(conn: sqlite3.Connection) -> tuple[str, int]:
    """Streaming fingerprint of application schema, reports and settings."""
    h = hashlib.sha256()
    records = 0
    for sql in (
        'SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name,tbl_name',
        'SELECT id,report_type,period_start,period_end,source_name,typeof(json),json '
        'FROM reports ORDER BY id',
        'SELECT name,typeof(json),json FROM kv ORDER BY name',
    ):
        h.update(b'\\xffQUERY\\x00' + sql.encode('utf-8'))
        for row in conn.execute(sql):
            for value in row:
                if value is None:
                    h.update(b'N')
                else:
                    data = value if isinstance(value, bytes) else str(value).encode('utf-8')
                    h.update(b'V' + len(data).to_bytes(8, 'big') + data)
            h.update(b'\\xfeROW\\x00')
            records += 1
    return h.hexdigest(), records


def recover_copy(source: Path, destination: Path) -> dict:
    """Create and verify a new independent SQLite file with no overwrite."""
    source = Path(source).absolute()
    destination = Path(destination).absolute()
    if source == destination:
        raise ValueError('Source and destination must differ')
    if source.suffix.lower() != '.sqlite3' or destination.suffix.lower() != '.sqlite3':
        raise ValueError('Expected .sqlite3 source and destination')
    if not source.is_file() or source.is_symlink():
        raise ValueError('Source must be an existing regular SQLite file')
    if not destination.parent.is_dir():
        raise ValueError('Destination directory does not exist')
    if os.path.lexists(destination):
        raise FileExistsError('Restore destination already exists: ' + str(destination))

    temporary = None
    try:
        with closing(_read_only(source)) as src:
            # The verification fingerprint and copied snapshot share a read view.
            src.execute('BEGIN')
            _healthy(src)
            expected, record_count = _digest(src)
            fd, name = tempfile.mkstemp(
                prefix='.analiz-restore-', suffix='.sqlite3', dir=destination.parent
            )
            temporary = Path(name)
            os.close(fd)
            with closing(sqlite3.connect(str(temporary))) as target:
                src.backup(target, pages=64, sleep=0.01)
            # Fold copied WAL into the independent file before publishing.
            with closing(sqlite3.connect(str(temporary))) as stage:
                stage.execute('PRAGMA wal_checkpoint(TRUNCATE)')
                mode = stage.execute('PRAGMA journal_mode=DELETE').fetchone()[0]
                if mode != 'delete':
                    raise ValueError('Unable to finalize restored SQLite file')
            src.rollback()

        with closing(_read_only(temporary)) as restored:
            _healthy(restored)
            actual, actual_count = _digest(restored)
        if actual != expected or actual_count != record_count:
            raise ValueError('Restored database differs from source snapshot')

        with temporary.open('rb') as file:
            os.fsync(file.fileno())
        # Atomic publication: os.link never replaces an existing destination.
        os.link(temporary, destination)
        return {
            'destination': str(destination),
            'sha256_logical': actual,
            'records': actual_count,
        }
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
            for suffix in ('-wal', '-shm'):
                Path(str(temporary) + suffix).unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(
        description='Rehearse SQLite recovery into a NEW file (no overwrite).'
    )
    parser.add_argument('source', type=Path, help='Existing SQLite backup')
    parser.add_argument('destination', type=Path, help='New .sqlite3 file; must not exist')
    args = parser.parse_args()
    try:
        result = recover_copy(args.source, args.destination)
    except (OSError, sqlite3.Error, ValueError) as exc:
        print('Recovery refused: ' + str(exc), file=sys.stderr)
        return 1
    print('Recovery copy verified: ' + json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == '__main__':
    sys.exit(main())
