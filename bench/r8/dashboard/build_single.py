"""Fold the 20 explorer snapshots into one self-navigating page (hash tabs, no cross-file links)."""
import re, sys
D = sys.argv[1]; OUT = sys.argv[2]
tabs = ['overview','scaling','table','steady','input','resize','footprint','compare','explore','runs']
def body(path):
    t = open(f'{D}/{path}').read()
    j = t.find('<div class="gx-static-root">')
    t = t[j:]
    return re.sub(r'\s*</body>\s*</html>\s*$', '', t)
head = open(f'{D}/table.html').read()
css = re.search(r'<style>.*?</style>', head, re.S).group(0)
font = re.search(r'<link rel="stylesheet" href="https://fonts[^>]*>', head).group(0)
secs = []
for run, pre in (('f', ''), ('u', 'u-')):
    for tab in tabs:
        key = f'{run}-{tab}'
        b = body(f'{pre}{tab}.html')
        b = b.replace('gx-clip-', f'gx-clip-{key}-')
        # tab links stay inside this run
        b = re.sub(r'href="(?:u-)?(%s)\.html"' % '|'.join(tabs), lambda m: f'href="#{run}-{m.group(1)}"', b)
        other = ('u' if run == 'f' else 'f')
        label = 'Unthrottled run' if run == 'f' else 'Fixed 60 Hz run'
        b = b.replace('<p class="gx-static-notice">', f'<p class="gx-static-notice"><a class="gx-runswitch" href="#{other}-{tab}" data-switch>View this tab in the {label}</a> &middot; ', 1)
        secs.append(f'<div class="gx-pg" id="pg-{key}" hidden>{b}</div>')
js = """<script>
(function(){
  var def='f-overview';
  function show(k){
    if(!document.getElementById('pg-'+k)) k=def;
    var all=document.querySelectorAll('.gx-pg');
    for(var i=0;i<all.length;i++) all[i].hidden=(all[i].id!=='pg-'+k);
    window.scrollTo(0,0);
  }
  function fromHash(){ show((location.hash||'').replace('#','')); }
  document.addEventListener('click',function(e){
    var a=e.target.closest&&e.target.closest('a[href^="#"]');
    if(!a) return;
    e.preventDefault();
    var k=a.getAttribute('href').slice(1);
    show(k);
    try{ history.replaceState(null,'','#'+k); }catch(_){}
  });
  window.addEventListener('hashchange',fromHash);
  fromHash();
})();
</script>"""
extra = '<style>.gx-runswitch{font-weight:600}</style>'
open(OUT,'w').write(f'<title>Bough Round 6 Results</title>{font}{css}{extra}' + ''.join(secs) + js)
print(len(secs))
