#!/usr/bin/env python3
"""index.html -> artifact.html: the page body in the form the Artifact publisher wraps (no doctype/html/head/body
tags, the side gutters it requires). Run after build.py."""
import re
from pathlib import Path
here = Path(__file__).parent
s = (here / 'index.html').read_text()
style = re.search(r'<style>(.*?)</style>', s, re.S).group(1)
body = re.search(r'<body>(.*?)</body>', s, re.S).group(1)
style = style.replace('main { max-width: 760px; margin: 0 auto; padding: 16px; }',
                      'main { max-width: 760px; margin: 0 auto; padding-block: 16px; padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px)); }')
style = style.replace('body {\n  margin: 0;', 'body {')
out = ('<title>Bough Round 6 Results</title>\n<meta name="claude:viewport-fit-sides" content="cover">\n'
       '<style>' + style + '</style>\n' + body.strip() + '\n')
(here / 'artifact.html').write_text(out)
print(len(out), 'bytes')
