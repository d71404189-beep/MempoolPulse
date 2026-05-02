import re, sys

v = sys.argv[1]

with open('src-tauri/tauri.conf.json', 'r') as f:
    c = f.read()
c = re.sub(r'"version": "[^"]+"', f'"version": "{v}"', c)
with open('src-tauri/tauri.conf.json', 'w') as f:
    f.write(c)

with open('src-tauri/Cargo.toml', 'r') as f:
    c = f.read()
c = re.sub(r'^version = "[^"]+"', f'version = "{v}"', c, count=1, flags=re.MULTILINE)
with open('src-tauri/Cargo.toml', 'w') as f:
    f.write(c)

print('Version set to:', v)
