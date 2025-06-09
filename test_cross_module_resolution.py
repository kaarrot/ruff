#!/usr/bin/env python3
"""
Test script to verify cross-module symbol resolution for goto definition.
This creates a test project structure and verifies that Ty can resolve symbols across modules.
"""

import os
import sys
import subprocess
import tempfile
import shutil
from pathlib import Path

def create_test_project(base_dir):
    """Create a test project with the structure described in the user's issue."""
    
    # Create directory structure
    base_path = Path(base_dir)
    
    # Create directories first
    (base_path / "a").mkdir(parents=True, exist_ok=True)
    (base_path / "a" / "b").mkdir(parents=True, exist_ok=True)
    (base_path / "d").mkdir(parents=True, exist_ok=True)
    
    # Create __init__.py files
    (base_path / "__init__.py").touch()
    (base_path / "a" / "__init__.py").touch()
    (base_path / "a" / "b" / "__init__.py").touch()
    
    # Create a/b/c.py with a ccc() function
    with open(base_path / "a" / "b" / "c.py", "w") as f:
        f.write("""
def ccc():
    \"\"\"This is the ccc function in a.b.c module.\"\"\"
    return "Hello from a.b.c.ccc!"

class CccClass:
    \"\"\"This is the CccClass in a.b.c module.\"\"\"
    def method(self):
        return "Hello from CccClass.method!"

CONSTANT_CCC = "constant from a.b.c"
""")
    
    # Create d/main.py that imports and uses a.b.c
    with open(base_path / "d" / "main.py", "w") as f:
        f.write("""
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
""")
    
    # Create pyproject.toml for proper project configuration
    with open(base_path / "pyproject.toml", "w") as f:
        f.write("""
[tool.ty]
[tool.ty.src]
root = "."

[tool.ty.environment]
python-version = "3.11"
extra-paths = ["."]
""")
    
    print(f"✅ Created test project at: {base_path}")
    return base_path

def test_ty_functionality(project_path, ruff_dir):
    """Test that Ty can check the project without errors."""
    try:
        # Test basic type checking - run cargo from ruff_dir but check project_path
        result = subprocess.run(
            ["cargo", "run", "--bin", "ty", "--", "check", str(project_path / "d" / "main.py")],
            cwd=ruff_dir,  # Run cargo from the ruff directory
            capture_output=True,
            text=True,
            timeout=30
        )
        
        print(f"✅ Ty check exit code: {result.returncode}")
        if result.stdout:
            print(f"📝 Stdout: {result.stdout}")
        if result.stderr:
            print(f"⚠️  Stderr: {result.stderr}")
            
        return result.returncode == 0
        
    except subprocess.TimeoutExpired:
        print("❌ Ty check timed out")
        return False
    except Exception as e:
        print(f"❌ Error running Ty: {e}")
        return False

def main():
    """Main test function."""
    print("🧪 Testing Cross-Module Symbol Resolution in Ty")
    print("=" * 50)
    
    # Get the current ruff directory
    ruff_dir = Path.cwd()
    
    # Create temporary test project
    with tempfile.TemporaryDirectory() as temp_dir:
        try:
            project_path = create_test_project(temp_dir)
            
            # Test Ty functionality
            success = test_ty_functionality(project_path, ruff_dir)
            
            if success:
                print("\n✅ SUCCESS: Cross-module resolution test passed!")
                print("\nTo manually test goto definition:")
                print(f"1. cd {project_path}")
                print("2. Open d/main.py in your editor with Ty LSP")
                print("3. Try 'go to definition' on 'ccc' in 'a.b.c.ccc()'")
                print("4. It should jump to the ccc() function in a/b/c.py")
            else:
                print("\n❌ FAILURE: Cross-module resolution test failed!")
                
        except Exception as e:
            print(f"❌ Test failed with error: {e}")
            return False
    
    return success

if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1) 