
# Test importing from different c.py modules
from ..a.c import function_in_a_c, ClassInAC, CONSTANT_FROM_A_C
from ..a.b.c import function_in_a_b_c, ClassInABC, CONSTANT_FROM_A_B_C

# Also test absolute imports
import a.c as ac
import a.b.c as abc

def test_function():
    # Using imports
    result1 = function_in_a_c()
    result2 = function_in_a_b_c()
    
    # Using classes
    obj1 = ClassInAC()
    obj2 = ClassInABC()
    
    # Using constants
    const1 = CONSTANT_FROM_A_C
    const2 = CONSTANT_FROM_A_B_C
    
    # Using via module references
    result3 = ac.function_in_a_c()
    result4 = abc.function_in_a_b_c()
    
    return result1, result2, const1, const2, result3, result4
