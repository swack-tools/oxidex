"""Source-owned numeric rows follow declarations, never tag-name exceptions."""
import copy
import json
from pathlib import Path
import unittest
from final_scalar_stage import _admitted_row, FinalStageRefused

class NumericFinalAdmissionTests(unittest.TestCase):
    def row(self):
        return json.loads(Path(__file__).with_name('testdata').joinpath('numeric_final_row.json').read_text())

    def test_copied_native_address_name_and_format_operands_propagate(self):
        row=self.row()
        row['properties']['Name']['value']='UnlistedNumericName'
        self.assertEqual(_admitted_row('777',row,{'int16u','rational64u'}), (777,'UnlistedNumericName','IFD0'))
        for owner in ('properties','write_controls'):
            row[owner]['Writable']['value']='int16u'
        self.assertEqual(_admitted_row('778',row,{'int16u','rational64u'}), (778,'UnlistedNumericName','IFD0'))
        for owner in ('properties','write_controls'):
            row[owner]['WriteGroup']['value']='ExifIFD'
        self.assertEqual(_admitted_row('778',row,{'int16u','rational64u'})[-1], 'ExifIFD')

    def test_copied_control_additions_mismatches_and_unsupported_formats_refuse(self):
        for name in ('Format','ValueConvInv','RawConvInv','WriteCheck','Count','Permanent','Condition'):
            row=self.row();row['properties'][name]={'present':True,'value':'1'}
            with self.subTest(control=name),self.assertRaises(FinalStageRefused):
                _admitted_row('777',row,{'int16u','rational64u'})
        row=self.row();row['write_controls']['Mandatory']['value']='0'
        with self.assertRaisesRegex(FinalStageRefused,'projections differ'):
            _admitted_row('777',row,{'int16u','rational64u'})
        row=self.row()
        for owner in ('properties','write_controls'):
            row[owner]['Writable']['value']='double'
        with self.assertRaises(FinalStageRefused):
            _admitted_row('777',row,{'int16u','rational64u'})

if __name__=='__main__':unittest.main()
