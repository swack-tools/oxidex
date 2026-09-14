import unittest
from convinv_recipes import scalar_fallthrough, ConvInvRecipe
from checkexif_recipes import RecipeRefused

class ScalarFallthrough(unittest.TestCase):
 def test_scalar_states_and_presence_refusal(self):
  r=ConvInvRecipe(None)
  for value in (b'a\0', 'é\0', None, ''): self.assertEqual(scalar_fallthrough(r,value,{}),(value,None))
  for key in ('PrintConv','PrintConvInv','ValueConv','ValueConvInv'):
   with self.subTest(key=key):
    with self.assertRaises(RecipeRefused): scalar_fallthrough(r,'x',{key:None})
 def test_gates_refuse(self):
  r=ConvInvRecipe(None)
  for row in ({'List':1},{'RawJoin':1},{'WriteCheck':1},{'RawConvInv':1},{'Table':{'CHECK_PROC':1}}):
   with self.assertRaises(RecipeRefused): scalar_fallthrough(r,'x',row)
if __name__=='__main__': unittest.main()
