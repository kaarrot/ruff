"""
Module a.b.c with functions that should be resolvable via goto definition.
"""

def ccc():
    """This is the ccc function in a.b.c module."""
    return "Hello from a.b.c.ccc!"

class CccClass:
    """This is the CccClass in a.b.c module."""
    def method(self):
        return "Hello from CccClass.method!"

CONSTANT_CCC = "constant from a.b.c" 