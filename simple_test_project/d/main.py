"""
Test file for cross-module symbol resolution.
Try goto definition on ccc, CccClass, and CONSTANT_CCC.
"""

import a.b.c

def test_cross_module():
    # This should resolve ccc() to a/b/c.py
    result = a.b.c.ccc()
    
    # This should resolve CccClass to a/b/c.py
    obj = a.b.c.CccClass()
    
    # This should resolve CONSTANT_CCC to a/b/c.py
    const = a.b.c.CONSTANT_CCC
    
    return result, obj, const

if __name__ == "__main__":
    print(test_cross_module()) 