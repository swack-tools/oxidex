import unittest
from audit_hydrated_layouts import audit, check_profile, check_catalog


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

    def test_profile_rejects_self_consistent_row_loss(self):
        source = document()
        expected = audit(source)
        source['hydrated_layouts']['tables']['Table'].update(tags={}, tag_count=0)
        with self.assertRaisesRegex(ValueError, 'profile mismatch: raw_keys'):
            check_profile(audit(source), expected)

    def test_profile_checks_source_and_perl_provenance(self):
        expected = audit(document())
        for key in ['producer_sha256', 'perl_version', 'sources']:
            actual = audit(document())
            actual['source_provenance'][key] = 'changed'
            with self.assertRaisesRegex(ValueError, 'source_provenance'):
                check_profile(actual, expected)

    def test_catalog_checks_names_coordinates_and_denominator(self):
        source = document()
        def catalog():
            return {'exiftool_version': '13.59',
                    'counts': {'catalog_total_tag_entries': 1},
                    'producer': {'sources': {'fixture': {}}},
                    'entries': [{'table': 'Table', 'raw_key': '_id',
                                 'variant_index': 0, 'name': 'Name'}]}
        check_catalog(source, catalog())
        mutations = [lambda c: c['entries'][0].update(name='name'),
                     lambda c: c['entries'][0].update(variant_index=-1),
                     lambda c: c['entries'][0].update(raw_key='missing'),
                     lambda c: c['entries'].clear(),
                     lambda c: c['producer']['sources'].clear()]
        for mutate in mutations:
            c = catalog()
            mutate(c)
            with self.assertRaises(ValueError):
                check_catalog(source, c)


if __name__ == '__main__':
    unittest.main()
