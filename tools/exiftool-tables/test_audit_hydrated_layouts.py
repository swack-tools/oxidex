import unittest
from audit_hydrated_layouts import audit


def document():
    return {'exiftool_version': '13.59', 'hydrated_layouts': {
        'schema': 'oxidex_hydrated_layout_projection_v1', 'selection': 'full_hydrated_catalog',
        'table_count': 1, 'requested_table_count': 1, 'available_table_count': 1,
        'tables': {'Table': {'full_name': 'Table', 'tag_count': 1, 'tags': {
            '_id': {'Name': 'Name', 'Table': {'__ref': 'tag_table', 'table_full_names': ['Table']},
                    'Groups': {'__ref': 'HASH', 'object_id': 'groups'}}}}},
        'shared_reference_objects': {'groups': {'kind': 'HASH', 'properties': {}}},
        'source_provenance': {'sources': {'fixture': {}}, 'producer_sha256': '0' * 64},
        'catalog_counts': {'total_tag_entries': 1, 'unique_tag_names': 1},
        'helpers': {'shortcuts': {'entry_count': 0}},
    }}


class AuditTests(unittest.TestCase):
    def test_counts_nested_references_and_preserves_special_keys(self):
        result = audit(document())
        self.assertEqual(result['reference_occurrences_by_kind'], {'HASH': 1, 'tag_table': 1})
        self.assertEqual(result['raw_keys'], 1)

    def test_refuses_dangling_table_or_object_binding(self):
        for mutate in [lambda h: h['shared_reference_objects'].clear(),
                       lambda h: h['tables']['Table']['tags']['_id']['Table'].update(table_full_names=['Absent'])]:
            source = document()
            mutate(source['hydrated_layouts'])
            with self.assertRaisesRegex(ValueError, 'reference'):
                audit(source)

    def test_subset_and_missing_table_cannot_claim_full_capture(self):
        for field, value in [('selection', 'explicit_full_name_subset'), ('available_table_count', 2)]:
            source = document()
            source['hydrated_layouts'][field] = value
            with self.assertRaises(ValueError):
                audit(source)


if __name__ == '__main__':
    unittest.main()
