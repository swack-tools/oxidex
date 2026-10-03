import unittest
from lib.config import cargo_env_value


class ConfigTests(unittest.TestCase):
    def test_string_and_structured_cargo_env_values(self):
        self.assertEqual(cargo_env_value({'KEY':'project'},'KEY'),'project')
        self.assertEqual(cargo_env_value({'KEY':{'value':'project','force':True}},'KEY'),'project')

    def test_invalid_structured_value_is_named(self):
        with self.assertRaisesRegex(ValueError,'KEY.*string value'):
            cargo_env_value({'KEY':{'force':True}},'KEY')
