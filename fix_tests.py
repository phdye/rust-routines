#!/usr/bin/env python3
"""
Fix rust-routines tests by replacing tokio::test with standard test
and wrapping async code in block_on from futures executor
"""

import os
import re
import sys

def fix_test_file(filepath):
    """Fix a single test file by replacing tokio::test"""
    
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # Check if file has tokio::test
    if '#[tokio::test]' not in content:
        return False
    
    print(f"Fixing {filepath}...")
    
    # Add necessary imports if not present
    if 'use futures::executor::block_on;' not in content:
        # Find where to add the import (after other use statements)
        import_pos = content.find('use ')
        if import_pos != -1:
            # Find the end of use statements block
            lines = content.split('\n')
            for i, line in enumerate(lines):
                if line.startswith('use '):
                    last_use_line = i
            
            # Insert the import after the last use statement
            lines.insert(last_use_line + 1, 'use futures::executor::block_on;')
            content = '\n'.join(lines)
    
    # Replace #[tokio::test] with #[test] and wrap async fn body
    pattern = r'#\[tokio::test\]\s*\n\s*async fn (\w+)\(\)'
    
    def replace_test(match):
        test_name = match.group(1)
        return f'#[test]\nfn {test_name}()'
    
    content = re.sub(pattern, replace_test, content)
    
    # Now we need to wrap the async test bodies in block_on
    # This is complex, so let's do it more carefully
    lines = content.split('\n')
    i = 0
    while i < len(lines):
        if '#[test]' in lines[i] and i + 1 < len(lines):
            # Check if next line is a fn that was async
            fn_line = lines[i + 1]
            if 'fn ' in fn_line and '()' in fn_line:
                # Find the opening brace
                brace_line = i + 1
                while brace_line < len(lines) and '{' not in lines[brace_line]:
                    brace_line += 1
                
                if brace_line < len(lines):
                    # Add block_on wrapper
                    indent = '    '
                    lines[brace_line] = lines[brace_line].replace('{', '{\n    block_on(async {')
                    
                    # Find the closing brace for this function
                    brace_count = 1
                    j = brace_line + 1
                    while j < len(lines) and brace_count > 0:
                        for char in lines[j]:
                            if char == '{':
                                brace_count += 1
                            elif char == '}':
                                brace_count -= 1
                                if brace_count == 0:
                                    # Found the closing brace
                                    lines[j] = lines[j].replace('}', '    })\n}', 1)
                                    break
                        j += 1
        i += 1
    
    content = '\n'.join(lines)
    
    # Write back the fixed content
    with open(filepath, 'w', encoding='utf-8') as f:
        f.write(content)
    
    return True

def main():
    test_dir = r'C:\-\cygwin\root\home\phdyex\my-repos\rust-routines\tests'
    
    # Find all .rs files in tests directory
    test_files = []
    for root, dirs, files in os.walk(test_dir):
        for file in files:
            if file.endswith('.rs'):
                test_files.append(os.path.join(root, file))
    
    fixed_count = 0
    for filepath in test_files:
        if fix_test_file(filepath):
            fixed_count += 1
    
    print(f"\nFixed {fixed_count} test files")

if __name__ == '__main__':
    main()
