"""Restore drills for v0.1.3 reports/kv schemas and uncheckpointed WAL."""
from contextlib import closing
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'tools' / 'restore_sqlite_copy.py'
spec = importlib.util.spec_from_file_location('restore_sqlite_copy', SCRIPT)
restore = importlib.util.module_from_spec(spec)
spec.loader.exec_module(restore)


class RecoveryCopyTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name)
        self.source = self.root / 'reports.sqlite3'
        self.destination = self.root / 'restored.sqlite3'
        self.live = sqlite3.connect(self.source)
        self.addCleanup(self.live.close)
        self.live.execute('PRAGMA journal_mode=WAL')
        self.live.execute('PRAGMA wal_autocheckpoint=0')
        self.live.executescript("""
            CREATE TABLE reports(id TEXT PRIMARY KEY,report_type TEXT NOT NULL,
                period_start TEXT NOT NULL,period_end TEXT NOT NULL,
                source_name TEXT,json TEXT NOT NULL);
            CREATE TABLE kv(name TEXT PRIMARY KEY,json TEXT NOT NULL);
            CREATE TABLE extra_table(value TEXT);
            CREATE INDEX reports_by_source ON reports(source_name);
        """)
        self.report = json.dumps({
            'id':'r1','type':'day','start':'2024-02-29','end':'2024-02-29',
            'rows':[{'work':'Ремонт','amount':123.5}],
        }, ensure_ascii=False)
        self.live.execute('INSERT INTO reports VALUES(?,?,?,?,?,?)', (
            'r1','day','2024-02-29','2024-02-29','quarter.xlsx',
            self.report.encode('utf-8'),
        ))
        self.live.execute('INSERT INTO kv VALUES(?,?)', (
            'settings',
            json.dumps({'orgNames':['СМУ16'],'groups':['Группа']},ensure_ascii=False),
        ))
        self.live.execute('INSERT INTO extra_table VALUES(?)',('must survive',))
        self.live.commit()

    def test_uncheckpointed_wal_and_blob_round_trip(self):
        self.assertTrue(Path(str(self.source)+'-wal').exists())
        result = restore.recover_copy(self.source, self.destination)
        self.assertTrue(result['sha256_logical'])
        with closing(sqlite3.connect(self.destination)) as db:
            self.assertEqual(db.execute('PRAGMA quick_check').fetchone(), ('ok',))
            raw = db.execute('SELECT json FROM reports WHERE id=?',('r1',)).fetchone()[0]
            self.assertEqual(json.loads(raw)['rows'][0]['amount'],123.5)
            self.assertIsInstance(raw,bytes)
            self.assertEqual(
                db.execute('SELECT value FROM extra_table').fetchone()[0], 'must survive'
            )
            self.assertIn('Группа',db.execute('SELECT json FROM kv').fetchone()[0])
        self.assertEqual(self.live.execute('SELECT COUNT(*) FROM reports').fetchone()[0],1)
        self.assertEqual(list(self.root.glob('.analiz-restore-*')), [])

    def test_existing_destination_remains_untouched(self):
        self.destination.write_bytes(b'KEEP')
        with self.assertRaises(FileExistsError):
            restore.recover_copy(self.source,self.destination)
        self.assertEqual(self.destination.read_bytes(),b'KEEP')

    def test_corrupted_backup_refused_without_file(self):
        self.live.close()
        broken=self.root/'broken.sqlite3'
        broken.write_bytes(b'not sqlite')
        with self.assertRaises(sqlite3.DatabaseError):
            restore.recover_copy(broken,self.destination)
        self.assertFalse(self.destination.exists())

    def test_non_application_db_refused_without_file(self):
        unrelated=self.root/'other.sqlite3'
        with closing(sqlite3.connect(unrelated)) as conn:
            conn.execute('CREATE TABLE unrelated(x)')
        with self.assertRaises(ValueError):
            restore.recover_copy(unrelated,self.destination)
        self.assertFalse(self.destination.exists())

    def test_json_corruption_refused_without_file(self):
        self.live.execute("UPDATE reports SET json='broken' WHERE id='r1'")
        self.live.commit()
        with self.assertRaises(ValueError):
            restore.recover_copy(self.source,self.destination)
        self.assertFalse(self.destination.exists())

    def test_publish_failure_cleans_staging_and_leaves_source(self):
        with patch.object(restore.os,'link',side_effect=OSError('hard link unavailable')):
            with self.assertRaises(OSError):
                restore.recover_copy(self.source,self.destination)
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.root.glob('.analiz-restore-*')), [])
        self.assertEqual(self.live.execute('SELECT COUNT(*) FROM reports').fetchone()[0],1)

    def test_same_source_or_wrong_suffix_rejected(self):
        with self.assertRaises(ValueError):
            restore.recover_copy(self.source,self.source)
        with self.assertRaises(ValueError):
            restore.recover_copy(self.source,self.root/'restored.json')


if __name__ == '__main__':
    unittest.main()
