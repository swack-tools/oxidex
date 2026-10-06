import unittest
import json
from pathlib import Path
import tempfile
from unittest.mock import patch
from lib.config import approved_instances, cargo_env_value


class ConfigTests(unittest.TestCase):
    def test_approval_is_disabled_until_deployment_is_verified(self):
        row={'name':'approved-bench','id':'278','project':'project','zone':'zone','enabled':False}
        with tempfile.TemporaryDirectory() as directory, patch('lib.config.ops_root',return_value=Path(directory)):
            path=Path(directory)/'config'/'remote-build.json'
            path.parent.mkdir()
            path.write_text(json.dumps({'schema_version':1,'instances':[row]}))
            self.assertEqual(approved_instances(),[])
            row['enabled']=True
            path.write_text(json.dumps({'schema_version':1,'instances':[row]}))
            self.assertEqual(approved_instances(),[row])
            path.write_text(json.dumps({'schema_version':1,'instances':[row,row]}))
            with self.assertRaisesRegex(ValueError,'Duplicate'):
                approved_instances()

    def test_string_and_structured_cargo_env_values(self):
        self.assertEqual(cargo_env_value({'KEY':'project'},'KEY'),'project')
        self.assertEqual(cargo_env_value({'KEY':{'value':'project','force':True}},'KEY'),'project')

    def test_invalid_structured_value_is_named(self):
        with self.assertRaisesRegex(ValueError,'KEY.*string value'):
            cargo_env_value({'KEY':{'force':True}},'KEY')
