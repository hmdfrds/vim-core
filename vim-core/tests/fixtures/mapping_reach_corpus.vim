" Configs that define mappings, for the test that presses each mapping
" and checks that every command line it runs is one mapping_reach
" predicted. A block ends at a line `"---`. In a block, `" text:` gives
" the document, with \n for a line break.
"---
nnoremap j gj
nnoremap k gk
"---
nnoremap j gj
"---
nnoremap k gk
"---
nnoremap <Leader>r :!python %<CR>
"---
nnoremap
"---
nnoremap <Leader>x :call system('rm -rf /')<CR>
"---
nnoremap j gj
nnoremap <Leader>r :!cargo run<CR>
set scrolloff=5
inoremap jk <Esc>
"---
nnoremap <Leader>r :!cargo
"---
inoremap jk <Esc>
"---
vnoremap <Leader>f :!sort<CR>
"---
nnoremap <Leader>e :call execute('!rm -rf /')<CR>
"---
nnoremap <Leader>s :echo shellescape(expand('%'))<CR>
"---
nnoremap <Leader>r :echo readfile('/etc/passwd')<CR>
"---
nnoremap <Leader>w :call writefile(['pwned'], '/tmp/evil')<CR>
"---
nnoremap <Leader>r :read !cat /etc/passwd<CR>
"---
nnoremap <Leader>r :r !cat /etc/passwd<CR>
"---
nnoremap <Leader>g :echo glob('*')<CR>
"---
nnoremap <Leader>p :echo getfperm('/etc/passwd')<CR>
"---
nnoremap <Leader>t :echo getftype('/tmp')<CR>
"---
nnoremap <Leader>s :echo getfsize('/etc/passwd')<CR>
"---
nnoremap <Leader>l :echo systemlist('whoami')<CR>
"---
nn <Leader>x :!rm -rf /<CR>
"---
no <Leader>x :!rm -rf /<CR>
"---
ino <Leader>x :!rm -rf /<CR>
"---
nm <Leader>x :!rm -rf /<CR>
"---
nnoremap! <Leader>x !ls<CR>
"---
nn! <Leader>x !ls<CR>
"---
map! <Leader>x !ls<CR>
"---
cmap <Leader>r <C-r>=system('whoami')<CR>
"---
cnoremap <Leader>x !ls<CR>
"---
cnoremap <C-x> :!ls<CR>
"---
nnoremap x ::!ls<CR>
"---
nnoremap x <Cmd>:!ls<CR>
"---
nnoremap x :noh<Bar>:so f<CR>
"---
nnoremap <Leader>s :source res://evil.txt<CR>
"---
nnoremap <Leader>s :so res://evil.txt<CR>
"---
nnoremap x :source<Tab>user://a.vim<CR>
"---
nnoremap x :source<Space>user://a.vim<CR>
"---
nnoremap x :so<S-Space>user://a.vim<CR>
"---
nnoremap x :sou	user://a.vim<CR>
"---
nnoremap x :so<C-I>user://a.vim<CR>
"---
nnoremap x :so1<CR>
"---
nnoremap x :source!<Space>user://a.vim<CR>
"---
nnoremap x :so<CR>
"---
nnoremap x :source
"---
nnoremap x :noh<Bar>so user://a.vim<CR>
"---
nnoremap x :noh\|so user://a.vim<CR>
"---
nnoremap x :silent so user://a.vim<CR>
"---
nnoremap x :silent!so user://a.vim<CR>
"---
nnoremap x :sil so user://a.vim<CR>
"---
nnoremap x :keepjumps so user://a.vim<CR>
"---
nnoremap x :2verbose so user://a.vim<CR>
"---
nnoremap x :windo so user://a.vim<CR>
"---
nnoremap x :<Space>so<Space>user://a.vim<CR>
"---
nnoremap x f/:so user://a.vim<CR>
"---
nnoremap x r?:so user://a.vim<CR>
"---
nnoremap x :<Tab>so user://a.vim<CR>
"---
nnoremap x :	so user://a.vim<CR>
"---
nnoremap <silent> x :so user://a.vim<CR>
"---
cnoremap <C-x> <Home>so<Space>user://a.vim
"---
nnoremap x <Cmd>source user://a.vim<CR>
"---
nnoremap x <cmd>silent so user://a.vim<CR>
"---
nnoremap x <Cmd>so<Space>user://a.vim<CR>
"---
nnoremap x <Cmd>noh<Bar>so user://a.vim<CR>
"---
nnoremap x <Cmd>!ls<CR>
"---
nnoremap x :urce user://a.vim<Home>so<CR>
"---
nnoremap x :o user://a.vim<Home>s<CR>
"---
nnoremap x :soX<BS> user://a.vim<CR>
"---
nnoremap x :sox<C-h> user://a.vim<CR>
"---
nnoremap x :silent !ls<CR>
"---
nnoremap x :noh<Bar>!ls<CR>
"---
nnoremap x :r	!ls<CR>
"---
nnoremap x :'a,'b!sort<CR>
"---
nnoremap x :/foo/!sort<CR>
"---
nnoremap x : !echo hi<CR>
"---
nnoremap x :　!echo hi<CR>
"---
nnoremap x : so f<CR>
"---
nnoremap x :r !id<CR>
"---
nnoremap x :1, 2!sort<CR>
"---
nnoremap x <Cmd>!ls<CR>
"---
nnoremap x :nnoremap y :!echo hi<lt>CR><CR>
"---
nnoremap x :nmap y Z<CR>
"---
nnoremap x :nunmap j<CR>
"---
nnoremap x :mapclear<CR>
"---
nnoremap x :iabbrev teh the<CR>
"---
nnoremap x :unabbreviate teh<CR>
"---
nnoremap x :abclear<CR>
"---
nnoremap x :sethandler n:ide<CR>
"---
nnoremap x :let mapleader = ','<CR>
"---
nnoremap x :set langmap=q:<CR>
"---
nnoremap x :setlocal shell=/bin/evil<CR>
"---
nnoremap x :setglobal kp?<CR>
"---
nnoremap x :noh<Bar>silent nnoremap y Z<CR>
"---
nnoremap x <Cmd>nnoremap y Z<CR>
"---
nnoremap x :windo nnoremap y Z<CR>
"---
nnoremap x :bufdo set mp=evil<CR>
"---
cnoremap <C-x> <C-u>nnoremap y Z<CR>
"---
nnoremap x :nnoremap y Z
"---
nnoremap <Leader>w :set wrap!<CR>
"---
nnoremap <Esc><Esc> :noh<CR>
"---
nnoremap x :echo 'hi'<CR>
"---
nnoremap x <Cmd>set hlsearch!<CR>
"---
nnoremap x :set so=5 tw=80 nonumber ic?<CR>
"---
nnoremap x :setlocal fo-=t<CR>
"---
nnoremap x :windo set wrap<CR>
"---
nnoremap x :r notes.txt<CR>
"---
nnoremap x :g/TODO/d<CR>
"---
nnoremap x :e res://main.gd<CR>
"---
nnoremap x :undefined<CR>
"---
nnoremap x :g/x/so user://a.vim<CR>
"---
nnoremap x :g!/x/so user://a.vim<CR>
"---
nnoremap x :v/x/so user://a.vim<CR>
"---
nnoremap x :g/x/!ls<CR>
"---
nnoremap x :v/x/r !ls<CR>
"---
nnoremap x :g/x/nnoremap y Z<CR>
"---
nnoremap x :v/x/set shell=/bin/evil<CR>
"---
nnoremap x :g/x/windo so f<CR>
"---
nnoremap x :g/x/v/y/so f<CR>
"---
nnoremap x :gl/x/so f<CR>
"---
nnoremap x :global/x/so f<CR>
"---
nnoremap x :global!/x/so f<CR>
"---
nnoremap x :vg/x/so f<CR>
"---
nnoremap x :vglobal/x/!ls<CR>
"---
nnoremap x <Cmd>g/x/so f<CR>
"---
nnoremap x :%g/x/so f<CR>
"---
nnoremap x :'<,'>v/x/so f<CR>
"---
nnoremap x :silent g/x/so f<CR>
"---
nnoremap x :windo g!/x/so f<CR>
"---
nnoremap x :noh<Bar>g/x/so f<CR>
"---
nnoremap x :noh\|v/x/!ls<CR>
"---
nnoremap x :g/x/silent so f<CR>
"---
nnoremap x :g/x/ :.,+1so f<CR>
"---
nnoremap x :g/a\/b/so f<CR>
"---
nnoremap x :g#x#so f<CR>
"---
nnoremap x :v,a\,b,!ls<CR>
"---
nnoremap x :g!x!so f<CR>
"---
nnoremap x :g /x/so f<CR>
"---
nnoremap x :g! /x/so f<CR>
"---
nnoremap x :g\/so f<CR>
"---
nnoremap x :g/a<Bar>b/so f<CR>
"---
nnoremap x :g/x/d<CR>
"---
nnoremap x :g/x/s/a/b/<CR>
"---
nnoremap x :g!/x/d<CR>
"---
nnoremap x :v/x/normal dd<CR>
"---
nnoremap x :g/so f/d<CR>
"---
nnoremap x :g/a\/so f/d<CR>
"---
nnoremap x :g/so f<CR>
"---
nnoremap x :goto 5<CR>
"---
nnoremap x :grep TODO<CR>
"---
nnoremap x :vsplit<CR>
"---
nnoremap x :vertical new<CR>
"---
nnoremap x :visual res://a.gd<CR>
"---
nnoremap x :view res://a.gd<CR>
"---
nnoremap x :vnew<CR>
"---
nnoremap x :sx/x/so user://a.vim<CR>
"---
nnoremap x :sy/x/!ls<CR>
"---
nnoremap x :sx#x#nnoremap y Z<CR>
"---
nnoremap x :sy/x/set shell=/bin/evil<CR>
"---
nnoremap x :silent sx/x/so f<CR>
"---
nnoremap x :g/x/sy/y/so f<CR>
"---
nnoremap x :windo sx/x/nnoremap y Z<CR>
"---
nnoremap x <Cmd>sy/x/so f<CR>
"---
nnoremap x :sx/x/d<CR>
"---
nnoremap x :sy/x/d<CR>
"---
nnoremap x :sx/so f/d<CR>
"---
nnoremap x :v/x/nnoremap y Z<CR>
"---
nnoremap x :global!/x/!ls<CR>
"---
nnoremap x /\v<(g<Bar>v<Bar>sx<Bar>sy<Bar>windo<Bar>bufdo<Bar>tabdo<Bar>g<Bar>v)><CR>
"---
nnoremap x :s/\v<(g<Bar>v<Bar>sx<Bar>sy<Bar>windo<Bar>bufdo<Bar>tabdo<Bar>g<Bar>v)>//<CR>
"---
nnoremap x /\%(foo\|bar\)\(baz\)<CR>
"---
nnoremap x ?fu\%[nction]<CR>
"---
nnoremap x d/\v<end%[if]><CR>
"---
nnoremap x /foo<BS><BS>ar<Home>\<<End>\><CR>
"---
nnoremap <Leader>s /\v
"---
inoremap <C-l> /* */<Left><Left><Left>
"---
vnoremap // y/<C-r>"<CR>
"---
nnoremap x /<C-r><C-w><CR>
"---
nnoremap x ?\<<C-r><C-a>\><CR>
"---
nnoremap x /<C-r>/<BS><CR>
"---
nnoremap x /<C-r><Esc>a<CR>
"---
nnoremap x :windo set nu<C-u>noh<CR>
"---
nnoremap x :g/x/d<BS>s/a/b/<CR>
"---
nnoremap x :bufdo e<Esc>:noh<CR>
"---
nnoremap x :tabdo windo set nu<Home><C-k>noh<CR>
"---
nnoremap x :%s/a/b/g<Left><Left><BS>c<CR>
"---
nnoremap x <Cmd>windo set nu<Esc><Cmd>noh<CR>
"---
cnoremap <C-x> <C-u>windo set nu
"---
nnoremap x :g/^$/d<CR>
"---
nnoremap x :windo set nu<CR>
"---
nnoremap x :tabdo windo set nu<CR>
"---
nnoremap x :silent keepjumps g/x/d<CR>
"---
nnoremap x :silent! windo g/x/s/a/b/g<CR>
"---
nnoremap x :bufdo silent %s/a/b/ge<CR>
"---
nnoremap x <Cmd>windo diffthis<CR>
"---
nnoremap x :%s/\%(foo\|bar\)\(baz\)/\1/g<CR>
"---
nnoremap x :g/\%(a\|b\)/s/\v%(c\|d)+/e/<CR>
"---
nnoremap x :%s/fu\%[nction]/func/g<CR>
"---
nnoremap x :g/\v<end%[if]>/d<CR>
"---
nnoremap x :s/\(\(\(\(\(\(\(\(\(a\)\)\)\)\)\)\)\)\)/\9/<CR>
"---
nnoremap x :e res://debug/log/img/svg/config/rev/nav/dev/env/tag.gd<CR>
"---
nnoremap x :'é<CR>
"---
nnoremap x :1,'éd<CR>
"---
nnoremap x <Cmd>'é<CR>
"---
nnoremap x :'é
"---
nnoremap x :s/'é/e/<CR>
"---
nnoremap x :'a,'bs/é/e/<CR>
"---
nnoremap x :e f<Cmd>m<CR><C-u>so f<Cmd>m<CR><C-u>
"---
nnoremap x :e f<Cmd>m<CR>a<BS><C-u>so f<Cmd>m<CR><C-u>
"---
nnoremap x :sof<Cmd>m<CR><Left><Cmd>m<CR><C-u>
"---
nnoremap x :sof<Left><Cmd>m<CR><Right><Cmd>m<CR><C-u>
"---
nnoremap x :w<Cmd>m<CR><Bar>nnoremap y Z<CR>
"---
cnoremap x e res://
"---
nn y Z
"---
nnoremap x :!echo hi<CR>
"---
set langmap=q:
nnoremap x 0q!echo hi<CR>
"---
nnoremap x f/:!echo hi<CR>
"---
nnoremap x t/:!echo hi<CR>
"---
nnoremap x $F/:!echo hi<CR>
"---
nnoremap x $T/:!echo hi<CR>
"---
nnoremap x f?:!echo hi<CR>
"---
nnoremap x df/:!echo hi<CR>
"---
nnoremap x r/:!echo hi<CR>
"---
nnoremap x m/:!echo hi<CR>
"---
nnoremap x @/:!echo hi<CR>
"---
nnoremap x "/:!echo hi<CR>
"---
nnoremap x `/:!echo hi<CR>
"---
nnoremap x '/:!echo hi<CR>
"---
nnoremap x $f/:!echo hi<CR>
"---
nnoremap x r?:!echo hi<CR>
"---
nnoremap x f/:nnoremap y :!echo hi<lt>CR><CR>
"---
nnoremap x f:w:!echo hi<CR>
"---
nnoremap x t:j:!echo hi<CR>
"---
nnoremap x wya:x:!echo hi<CR>
"---
nnoremap x c:xif:!echo hi<CR>
"---
nnoremap x d:T<Del>*w:<Left>!echo hi
"---
vnoremap x :<Bar>nnoremap y <lt>Cmd>!echo hi<lt>CR><CR>
"---
nnoremap x v:<Bar>nnoremap y <lt>Cmd>!echo hi<lt>CR><CR>
"---
cnoremap x <Bar>nnoremap y <lt>Cmd>!echo hi<lt>CR>
"---
nnoremap x :!echo hi<Cmd>m<CR><C-u>
"---
nnoremap x :!echo hi<Cmd>e!<CR><Esc>
"---
nnoremap x :noh!echo hi<Home><Right><Right><Right>
"---
nnoremap x f:w:so f<CR>
"---
nnoremap x f:w:nnoremap y Z<CR>
"---
nnoremap x t:j:set langmap=q:<CR>
"---
nnoremap x f:w:let mapleader=","<CR>
"---
nnoremap x <C-v>:<BS>so f<CR>
"---
nnoremap x <C-v>:<BS><C-w>nnoremap y Z<CR>
"---
vnoremap x :<BS>!echo hi<CR>
"---
nnoremap x <C-v>mT:<BS><C-w>nnoremap y  set langmap=q:<CR>
"---
nnoremap x :so f<Cmd>m<CR><C-u>
"---
nnoremap x :nnoremap y Z<Cmd>m<CR><C-u>
"---
nnoremap x :let mapleader=","<Cmd>m<CR><C-u>
"---
nnoremap x :set langmap=q:<Cmd>e!<CR><Esc>
"---
nnoremap x :sethandler <C-a> n:ide<Cmd>m<CR><C-u>
"---
vnoremap x :so f<Cmd>m<CR><C-u>
"---
nnoremap x :noh<Cmd>noh<CR><C-u>
"---
nnoremap x :<Cmd>noh<CR>
"---
nnoremap x :let mapleader=","<Bar>
"---
nnoremap x :set langremap<Bar>
"---
nnoremap x :set lrm<Bar>
"---
nnoremap x :setlocal langremap<Bar>
"---
nnoremap x :setglobal lrm<Bar>
"---
nnoremap x :sof<Left>
"---
nnoremap x :sof<Left><Cmd>m<CR><C-u>
"---
nnoremap x :let mapleader=","<Bar><Cmd>m<CR><C-u>
"---
nnoremap x <Cmd>let mapleader=","<Bar>
"---
vnoremap x :set langremap<Bar>
"---
nnoremap x yi':1set langremap<Home>:
"---
nnoremap x :%s//g<Left><Left>
"---
nnoremap x :e<Space>
"---
nnoremap x :ls<CR>:b<Space>
"---
nnoremap x :set<Space>
"---
nnoremap x :let<Space>
"---
nnoremap x :set tw=<Left>
"---
nnoremap x :vimgrep // **<Left><Left><Left><Left>
"---
vnoremap x :normal<Space>
"---
cnoremap <CR> so f<Home><Del><CR>
"---
cnoremap x so f<Home><Del>
"---
cnoremap x set langmap=q:<Home><Del>
"---
cnoremap x let mapleader=","<Home><Del>
"---
cnoremap x ,1let mapleader=","
"---
cnoremap x ,1nnoremap y Z
"---
cnoremap <CR> <Home>so f<CR>
"---
noremap! x so f<Home><Del>
"---
cnoremap <C-a> <Home>
"---
cnoremap <C-e> <End>
"---
cnoremap <C-b> <Left>
"---
cnoremap jj <Esc>
"---
cnoremap <C-x> <C-u>noh
"---
cnoremap <Tab> langmap=q:<CR>
"---
cnoremap <Tab> lrm
"---
cnoremap <Tab> mapleader=","<CR>
"---
cnoremap x ar !echo hi<CR>
"---
cnoremap x alet mapleader=","<CR>
"---
cnoremap x let mapleader=","<Bar>
"---
cnoremap x t mapleader=","<Bar>le
"---
cnoremap <C-x> <C-u>noh<CR>
"---
cnoremap <C-x> %s//g<Left><Left>
"---
cnoremap <C-l> <C-u>
"---
cnoremap W w
"---
noremap! jk <Esc>
"---
vnoremap x :<Bar>nnoremap y Z<CR>
"---
vnoremap x :<Bar>set langmap=q:<CR>
"---
vnoremap x :<Bar>let mapleader=","<CR>
"---
vnoremap x :<Space><Bar>nnoremap y Z<CR>
"---
vnoremap x :<Bar><Bar>nnoremap y Z<CR>
"---
vnoremap x :<Bar>silent nnoremap y Z<CR>
"---
vnoremap x :a<C-u><Bar>nnoremap y Z<CR>
"---
vnoremap x :?<C-h><Bar>nnoremap y Z<CR>
"---
vnoremap x :<Home><Bar>nnoremap y <CR>
"---
vnoremap x :<Home>let mapleader=<Right>,<Del><Del><Del><Del>'<CR>
"---
nnoremap x v:<Bar>nnoremap y Z<CR>
"---
cnoremap x <Bar>nnoremap y Z
"---
vnoremap x :sort<CR>
"---
vnoremap x :s/a/b/g<CR>
"---
vnoremap x :<C-u>noh<CR>
"---
vnoremap x :<BS><BS><BS><BS><BS>noh<CR>
"---
vnoremap x :<Home>silent <End>s/a/b/<CR>
"---
nnoremap x f:w
"---
nnoremap x f:C: 0<Esc>
"---
nnoremap x ":p
"---
inoremap <C-l> <Esc>f:a
"---
nnoremap x :%s/a:b/c/g<Left><Left>
"---
nnoremap x :%s/\v(\d+):(\d+)/\2:\1/g<CR>
"---
nnoremap x :e res://scenes/main.tscn<CR>
"---
nnoremap x :sort<CR>
"---
nnoremap x :set so=5<CR>
"---
nnoremap x :echo 'so far'<CR>
"---
nnoremap x :tabnew so.gd<CR>
"---
nnoremap x :retab<CR>
"---
nnoremap <Leader>so :w<CR>
"---
nnoremap x <C-w>so
"---
nnoremap x :w<CR><C-w>so
"---
nnoremap x :%s/a/so/g<Left><Left>
"---
nnoremap x <Cmd>noh<CR>
"---
nnoremap x <Cmd>w<CR>
"---
nnoremap x :echo<C-r>a<CR>
"---
nnoremap x :echo<Up><CR>
"---
nnoremap x :echo<C-f>
"---
cnoremap <C-x> <C-r>a
"---
nnoremap x ?<Up><CR>
"---
nnoremap x /<C-r>=@a<CR><CR>
"---
nnoremap <Leader>r :Read !cat /etc/passwd<CR>
"---
nnoremap <Leader>r :READ !cat /etc/passwd<CR>
"---
nnoremap <Leader>r :R !cat /etc/passwd<CR>
"---
nnoremap <Leader>s :Source evil.txt<CR>
"---
nnoremap <Leader>s :SOURCE evil.txt<CR>
"---
nnoremap <Leader>s :So evil.txt<CR>
"---
nnoremap <Leader>s :SOU evil.txt<CR>
"---
nnoremap <Leader>x :%!sort<CR>
"---
nnoremap <Leader>x :'<,'>!sort<CR>
"---
nnoremap <Leader>x :1,$!sort<CR>
"---
nnoremap <Leader>x :.!sh<CR>
"---
nnoremap <Leader>x :ñfoo<CR>
"---
nmap j gj
"---
nmap
"---
map j gj
"---
map j
"---
vmap <Leader>y "+y
"---
imap jk <Esc>
"---
omap p ip
"---
cmap <C-a> <Home>
"---
nm j gj
"---
vm <Leader>y "+y
"---
im jk <Esc>
"---
nmap! j gj
"---
noremap j gj
"---
vnoremap <Leader>y "+y
"---
onoremap p ip
"---
no j gj
"---
nn j gj
"---
vn <Leader>y "+y
"---
ino jk <Esc>
"---
ono p ip
"---
cno <C-a> <Home>
"---
nnoremap j gj
nmap k gk
set scrolloff=5
inoremap jk <Esc>
imap jj <Esc>
"---
nmap k gk
"---
imap jj <Esc>
"---
nnoremap x y | !id
"---
nnoremap x :echo 1 \| echo 2<CR>
"---
nnoremap j gj
nmap Q iBAD<Esc>
nnoremap K :!ls<CR>
set tw=10
set so=5 | !ls
"---
nnoremap x :e res://source/main.gd<CR>
"---
nnoremap x :e res://scripts/player.gd<CR>
"---
nnoremap x :e res://addons/a/plugin.gd<CR>
"---
nnoremap x :e res://src/main.gd<CR>
"---
nnoremap x :e res://shaders/a.gdshader<CR>
"---
nnoremap x :e res://sounds/a.ogg<CR>
"---
nnoremap x :e res://maps/a.tscn<CR>
"---
nnoremap x :e res://map/a.tscn<CR>
"---
nnoremap x :e res://sources/a.gd<CR>
"---
nnoremap x :e res://resources/a.tres<CR>
"---
nnoremap x :e res://ui/hud.tscn<CR>
"---
nnoremap x :e res://levels/one.tscn<CR>
"---
nnoremap x :e res://autoload/game.gd<CR>
"---
nnoremap x :e res://assets/a.png<CR>
"---
nnoremap x :e res://tests/test_a.gd<CR>
"---
nnoremap x :e res://set/a.gd<CR>
"---
nnoremap x :e res://norm/a.gd<CR>
"---
nnoremap x :e res://nodes/a.gd<CR>
"---
nnoremap x :e res://read/a.gd<CR>
"---
nnoremap x :e res://r/a.gd<CR>
"---
nnoremap x :e res://w/a.gd<CR>
"---
nnoremap x :e user://save.cfg<CR>
"---
nnoremap x :%s/:\s*/: /g<CR>
"---
vnoremap x :s/\s*:\s*/: /g<CR>
"---
nnoremap x :%s/\/\//\/\/ /g<CR>
"---
nnoremap x :g/^\s*#/d<CR>
"---
nnoremap x :%s/https:\/\/a\.b/c/g<CR>
"---
nnoremap x /https:\/\/<CR>
"---
nnoremap x /TODO:<CR>
"---
nnoremap x /FIXME:\\|TODO:<CR>
"---
nnoremap x :vimgrep /TODO:/ **/*.gd<CR>
"---
nnoremap x f:a<Space><Esc>
"---
nnoremap x 0f:lD
"---
nnoremap x f:r=
"---
nnoremap x t:x
"---
inoremap x <Esc>f:a
"---
inoremap x func ():<Left><Left><Left>
"---
inoremap x if :<CR>pass<Esc>
"---
inoremap x for i in range():<CR>pass<Esc>
"---
inoremap x var : int = 0<Esc>
"---
inoremap x # TODO: source the config<Esc>
"---
inoremap x # NOTE: map this later<Esc>
"---
inoremap x # FIXME: set it up<Esc>
"---
inoremap x # TODO: read the file<Esc>
"---
inoremap x # see https://docs.godotengine.org/en/stable/<Esc>
"---
inoremap x print("a: %s" % a)<Esc>
"---
nnoremap x o# TODO: <Esc>
"---
nnoremap x :echo "a:b:c"<CR>
"---
nnoremap x :let @/ = 'a:b'<CR>
"---
nnoremap x :set statusline=%f:%l:%c<CR>
"---
nnoremap x :e %:h/a.gd<CR>
"---
nnoremap x :e %:p:h<CR>
"---
nnoremap x :cd %:p:h<CR>
"---
nnoremap x :e <C-r>=expand('%:h')<CR>/
"---
nnoremap x :%s/\<<C-r><C-w>\>//g<Left><Left>
"---
nnoremap x */<C-r>/<CR>
"---
vnoremap x y:%s/<C-r>"//g<Left><Left>
"---
vnoremap x y/<C-r>"<CR>
"---
nnoremap x /\v<C-r><C-w><CR>
"---
nnoremap x :%s/a/b/g<CR>:w<CR>
"---
nnoremap x :w<CR>:!godot<CR>
"---
nnoremap x ?^func <CR>
"---
nnoremap x /^func \|^static func <CR>
"---
nnoremap x :g/a:b/s//c/<CR>
"---
nnoremap x :normal! f:D<CR>
"---
cnoremap w!! w !sudo tee % > /dev/null
"---
inoremap <C-k> // <C-o>A
"---
let mapleader = " "
"---
nnoremap <Leader>s <Action>(editor/save_scene)
"---
nnoremap <Leader>w :save<CR>
"---
nnoremap <Space>r :run<CR>
"---
vnoremap < <gv
"---
vnoremap > >gv
"---
cmap <C-V>		<C-R>+
"---
cmap <S-Insert>		<C-R>+
"---
cmap <SID><cfile> <Plug><cfile>
"---
cmap <SID><ctag>  <Plug><ctag>
"---
cmap <expr> <Plug><cfile> substitute(RubyCursorFile(),'^$',"\022\006",'')
"---
cmap <expr> <Plug><ctag> substitute(RubyCursorTag(),'^$',"\022\027",'')
"---
cmap <expr> <SID>tagzv &foldopen =~# 'tag' ? '<Bar>norm! zv' : ''
"---
cmap to that value if first matching encoding is going to be used.
"---
cnoremap <C-A> <C-C>gggH<C-O>G
"---
cnoremap <C-F4> <C-C><C-W>c
"---
cnoremap <C-Tab> <C-C><C-W>w
"---
cnoremap <M-Space> <C-C>:simalt ~<CR>
"---
cnoremap <SID>foldopen <Bar>if &foldopen =~# 'tag'<Bar>exe 'norm! zv'<Bar>endif
"---
cnoremap <expr> <C-F> has("gui_running") ? "\<C-\>\<C-C>:promptfind\<CR>" : "\<C-\>\<C-O>/"
"---
cnoremap <expr> <C-H> has("gui_running") ? "\<C-\>\<C-C>:promptrepl\<CR>" : "\<C-H>"
"---
cnoremap <expr> <Down> wildmenumode() ? "\<C-E>\<Down>" : "\<Down>"
"---
cnoremap <expr> <Up>   wildmenumode() ? "\<C-E>\<Up>"   : "\<Up>"
"---
cnoremap <silent> <C-F> <C-C>:promptfind<CR>
"---
cnoremap <silent> <CR> <CR>:cunmap <lt>CR><CR>zt<SID>L
"---
cnoremap K somethingelse2
"---
imap		   - Create an insert map
"---
imap <C-C>R <C-\><C-O>:call sqlcomplete#Map('ResetCache')<CR><C-X><C-O>
"---
imap <C-C>T <C-\><C-O>:call sqlcomplete#Map('sqlType')<CR><C-X><C-O>
"---
imap <C-C>a <C-\><C-O>:call sqlcomplete#Map('syntax')<CR><C-X><C-O>
"---
imap <C-C>f <C-\><C-O>:call sqlcomplete#Map('sqlFunction')<CR><C-X><C-O>
"---
imap <C-C>k <C-\><C-O>:call sqlcomplete#Map('sqlKeyword')<CR><C-X><C-O>
"---
imap <C-C>k <C-\><C-O>:call sqlcomplete#Map('sqlKeyword\w*')<CR><C-X><C-O>
"---
imap <C-C>o <C-\><C-O>:call sqlcomplete#Map('sqlOption')<CR><C-X><C-O>
"---
imap <C-C>s <C-\><C-O>:call sqlcomplete#Map('sqlStatement')<CR><C-X><C-O>
"---
imap <C-[> [[[
"---
imap <C-{> {{{
"---
imap <Down>   <C-o>gj
"---
imap <S-Down>   <C-o>gj
"---
imap <S-Insert>		<C-V>
"---
imap <S-Up>     <C-o>gk
"---
imap <Up>     <C-o>gk
"---
imap <leftmouse>             <Plug>ILeftmouse
"---
imap <leftmouse>           <Plug>ILeftmouse
"---
imap <middlemouse>           <Plug>IMiddlemouse
"---
imap <middlemouse>         <Plug>IMiddlemouse
"---
imap <s-leftmouse>         <Plug>ISLeftmouse
"---
inoremap # X#
"---
inoremap ' -
"---
inoremap ( ()<C-G>U<Left>
"---
inoremap + }
"---
inoremap , w
"---
inoremap - [
"---
inoremap . v
"---
inoremap / z
"---
inoremap : S
"---
inoremap ; s
"---
inoremap < W
"---
inoremap <C-A> <C-O>gg<C-O>gH<C-O>G
"---
inoremap <C-F4> <C-O><C-W>c
"---
inoremap <C-N> <C-R>=ada#Completion("\<lt>C-N>")<cr>
"---
inoremap <C-P> <C-R>=ada#Completion("\<lt>C-P>")<cr>
"---
inoremap <C-Q> <C-O>:confirm qall<CR>
"---
inoremap <C-S>		<Esc>:update<CR>gi
"---
inoremap <C-Tab> <C-O><C-W>w
"---
inoremap <C-U> <C-G>u<C-U>
"---
inoremap <C-V>      <C-R>"
"---
inoremap <C-X><C-]> <C-R>=<SID>ada#Completion("\<lt>C-X>\<lt>C-]>")<cr>
"---
inoremap <C-Y> <C-O><C-R>
"---
inoremap <C-Z> <C-O>u
"---
inoremap <Del> <BS>
"---
inoremap <F5> <C-R>=ListMonths()<CR>
"---
inoremap <F5> <ScriptCmd>ListMonths()<CR>
"---
inoremap <Left>  <C-G>U<Left>
"---
inoremap <M-Space> <C-O>:simalt ~<CR>
"---
inoremap <Right> <C-G>U<Right>
"---
inoremap <S-Insert> <C-R>"
"---
inoremap <Tab> <C-R>=CleverTab()<CR>
"---
inoremap <expr> . MayComplete()
"---
inoremap <expr> <C-F> has("gui_running") ? "\<C-\>\<C-O>:promptfind\<CR>" : "\<C-\>\<C-O>/"
"---
inoremap <expr> <C-H> has("gui_running") ? "\<C-\>\<C-O>:promptrepl\<CR>" : "\<C-H>"
"---
inoremap <expr> <C-L> ListItem()
"---
inoremap <expr> <C-L> nr2char(getchar())
"---
inoremap <expr> <C-L>x "foo"
"---
inoremap <expr> <C-R> ListReset()
"---
inoremap <expr> <End> repeat('<C-G>U<Right>', col('$') - col('.'))
"---
inoremap <expr> <Home> col('.') == match(getline('.'), '\S') + 1 ?
"---
inoremap <silent> <C-D> <C-R>=<SID>DecreaseIndent()<CR><C-R>=<SID>RestoreShiftwidth()<CR>
"---
inoremap <silent> <C-F> <C-O>:promptfind<CR>
"---
inoremap <silent> <C-T> <C-R>=<SID>IncreaseIndent()<CR><C-R>=<SID>RestoreShiftwidth()<CR>
"---
inoremap <silent> <CR> <Esc>:call <SID>CR()<CR>
"---
inoremap <silent> <Down> <C-R>=pumvisible() ? "\<lt>Down>" : "\<lt>C-O>gj"<CR>
"---
inoremap <silent> <Tab> <C-R>=<SID>Tab()<CR><C-R>=<SID>RestoreShiftwidth()<CR>
"---
inoremap <silent> <Up> <C-R>=pumvisible() ? "\<lt>Up>" : "\<lt>C-O>gk"<CR>
"---
inoremap <silent> <bs> <C-R>=ada#Insert_Backspace ()<cr>
"---
inoremap <silent><expr> <S-Tab> pumvisible() ? "\<C-p>" : "\<S-Tab>"
"---
inoremap <silent><expr> <Tab>   pumvisible() ? "\<C-n>" : "\<Tab>"
"---
inoremap = ]
"---
inoremap > V
"---
inoremap ? Z
"---
inoremap A A
"---
inoremap B X
"---
inoremap C J
"---
inoremap D E
"---
inoremap E >
"---
inoremap F U
"---
inoremap G I
"---
inoremap H D
"---
inoremap I C
"---
inoremap J H
"---
inoremap K T
"---
inoremap L N
"---
inoremap M M
"---
inoremap N B
"---
inoremap O R
"---
inoremap P L
"---
inoremap Q
"---
inoremap R P
"---
inoremap S O
"---
inoremap T Y
"---
inoremap U G
"---
inoremap V K
"---
inoremap W <
"---
inoremap X Q
"---
inoremap Y F
"---
inoremap Z :
"---
inoremap [ /
"---
inoremap ] =
"---
inoremap ^? ^H
"---
inoremap _ {
"---
inoremap a a
"---
inoremap b x
"---
inoremap c j
"---
inoremap d e
"---
inoremap e .
"---
inoremap f u
"---
inoremap g i
"---
inoremap h d
"---
inoremap i c
"---
inoremap j h
"---
inoremap k t
"---
inoremap l n
"---
inoremap m m
"---
inoremap n b
"---
inoremap o r
"---
inoremap p l
"---
inoremap q '
"---
inoremap r p
"---
inoremap s o
"---
inoremap t y
"---
inoremap u g
"---
inoremap v k
"---
inoremap w ,
"---
inoremap x q
"---
inoremap y f
"---
inoremap z ;
"---
inoremap { ?
"---
inoremap } +
"---
lnoremap <F8> <C-^>
"---
map & :%s/^\(-[ A-Z]*-\)\(-[ A-Z]*-\)\(-[.]*-\)$/\2\3\3/
"---
map ( ,s,i,X0i?^#A 
"---
map )0 o-                    --....................--....................-
"---
map )1 o-    JOHN CONWAY     --....................--....................-
"---
map )2 o-       LIVES        --....................--....................-
"---
map * 1G/INIT
"---
map + <1C<2
"---
map ,!
"---
map ,#
"---
map ,$ dd
"---
map ,%
"---
map ,& 0"gd$
"---
map ,* @g
"---
map ,- :s/./-/g
"---
map ,1 y G$p
"---
map ,2 2y G$p
"---
map ,3 3y G$p
"---
map ,@
"---
map ,A  oanother line<Esc>
"---
map ,IIN G?^top
"---
map ,Iaa o=AB =BC =CD =DE =EF =FG =GH =HI =IJ =JK =KL =LM =MN =NO =OP =PQ =QR
"---
map ,Iab o=RS =ST =TU =UV =VW =WX =XY =YZ =Z 
"---
map ,Id0 o#  .`a22lr 
"---
map ,Id1 o# X .`a22lr 
"---
map ,Id2 o# XX .`a22lr 
"---
map ,Id3 o# XXX .`a22lrA
"---
map ,Id4 o# XXXX .`a22lr 
"---
map ,Id5 o# XXXXX .`a22lr 
"---
map ,Id6 o# XXXXXX .`a22lr 
"---
map ,Id7 o# XXXXXXX .`a22lr 
"---
map ,Id8 o# XXXXXXXX .`a22lr 
"---
map ,Il0 o#XX .`a22lr 
"---
map ,Il1 o#XXX .`a22lr 
"---
map ,Il2 o#XXXX .,a
"---
map ,Il3 o#XXXXX .,a
"---
map ,Il4 o#XXXXXX .`a22lr 
"---
map ,Il5 o#XXXXXXX .`a22lr 
"---
map ,Il6 o#XXXXXXXX .`a22lr 
"---
map ,Il7 o#XXXXXXXXX .`a22lr 
"---
map ,Il8 O#XXXXXXXXXX .`a22lr 
"---
map ,R 03stop
"---
map ,X :.g/[A-Z]/s//X/g
"---
map ,Y
"---
map ,^ AiC
"---
map ,a `a,jGi?=,ma
"---
map ,c  <Plug>TypecorrAdd;
"---
map ,c c$
"---
map ,d
"---
map ,d :runtime dvorak/enable.vim<CR>
"---
map ,i `ay GP
"---
map ,j y
"---
map ,m p
"---
map ,n x
"---
map ,o oPut 'X's in the left box, then hit 'C' or 'R'
"---
map ,q :runtime dvorak/disable.vim<CR>
"---
map ,r makh,2`akF-l,1`ah,2`aF-l,1`ajh,2`ajF-l,1(
"---
map ,s :.g/ /s///g
"---
map ,w :call TermDebugSendCommand('where')<CR>
"---
map < g
"---
map <1 G?^top
"---
map <2 G?^top
"---
map << :r!date
"---
map <C-B> b
"---
map <C-D> d
"---
map <C-E> <CR>
"---
map <C-F> <Space>
"---
map <C-J> <CR>
"---
map <C-K> k
"---
map <C-N> <CR>
"---
map <C-P> k
"---
map <C-V>
"---
map <C-V> <Space>
"---
map <C-Y> k
"---
map <C-_> :cstag <C-R>=expand("<cword>")<CR><CR>
"---
map <Down> <CR>
"---
map <End> G
"---
map <Esc>< g
"---
map <Esc><Space> <Space>
"---
map <Esc>> G
"---
map <Esc>v b
"---
map <F19> :call BufNetWrite('something')<CR>
"---
map <F3> o#include	# insert include
"---
map <F3> o#include|# insert include
"---
map <F4> o#include
"---
map <Home> g
"---
map <Leader>A  oanother line<Esc>
"---
map <LocalLeader>c <Plug>SpecChangelog
"---
map <LocalLeader>i <Plug>JavaImport;
"---
map <PageDown> <Space>
"---
map <PageUp> b
"---
map <S-Down> <Space>
"---
map <S-Insert>
"---
map <S-Insert> <MiddleMouse>
"---
map <S-Up> b
"---
map <Up> k
"---
map <expr> <F11> &go =~# 's' ? ":se go-=s<CR>" : ":se go+=s<CR>"
"---
map <kEnd> G
"---
map <kHome> g
"---
map <kPageDown> <Space>
"---
map <kPageUp> b
"---
map <silent> <C-]> :call <SID>GprofJumpToFunctionIndex()<CR>
"---
map = 'kf,
"---
map = g:termdebug_map_K
"---
map = get(g:termdebug_config, 'map_K', true)
"---
map = get(g:termdebug_config, 'map_minus', true)
"---
map = get(g:termdebug_config, 'map_plus', true)
"---
map = true
"---
map > 0 LWWWWWWWWWWWWWWWWWW,rj
"---
map > G
"---
map A
"---
map B
"---
map B ,%k>,$
"---
map C
"---
map C T>>>>>>>>B&
"---
map Define a mapping local to the buffer.
"---
map E
"---
map F
"---
map F :e<CR>G<SID>L:sleep 1<CR>F
"---
map F yl
"---
map Give a warning if a mapping already exists.
"---
map H h
"---
map I G)0)0)0)0)1)0)0)2)0)0)0)0,ok,-11k,-,R,IIN
"---
map I KMYNOQNOSkRTV
"---
map J /^0[^t]*$
"---
map K 1Go
"---
map L 1G/t
"---
map L makf-h,1`ak,2`af-h,1`a,2`ajf-h,1`aj,2(
"---
map M dG
"---
map N yy
"---
map O p
"---
map P p
"---
map Q 0iT
"---
map Q gq
"---
map R $rn
"---
map R CV
"---
map S $r$
"---
map T G,c2k,!9k,@,#j>2k,$j
"---
map T ko0
"---
map T y$
"---
map U L
"---
map V Go/
"---
map V R
"---
map W makh,3`ah,3`ajh,3(
"---
map X x
"---
map Y o0123456789Z0q
"---
map \A  oanother line<Esc>
"---
map _n :n<Bar>execute "/\\<" .. g:word .. "\\>"<CR>
"---
map _u :call ID_search()<Bar>execute "/\\<" .. g:word .. "\\>"<CR>
"---
map command applies.  The mapping may remain defined
"---
map e
"---
map e <CR>
"---
map f <Space>
"---
map g 1G/^[(as;.]
"---
map g IL
"---
map g IR
"---
map g<C-\> :cs find 0 <C-R>=expand("<cword>")<CR><CR>
"---
map g<C-]> :cs find 3 <C-R>=expand("<cword>")<CR><CR>
"---
map j <CR>
"---
map p %
"---
map q tllD
"---
map t 7
"---
map v :silent call <SID>End()<CR>
"---
map w b
"---
map y k
"---
map z ,^,&,*,&<1,*<2
"---
map z <Space>
"---
map! <S-Insert> <MiddleMouse>
"---
map! t 7
"---
nmap ! f!<Ignore>x
"---
nmap ! f!x
"---
nmap (whatever)	<Plug>NetrwShrink
"---
nmap ,gq :%s/\s\+/ /g<CR>gq1G
"---
nmap <2-leftmouse>           <Plug>Netrw2Leftmouse
"---
nmap <2-leftmouse>         <Plug>Netrw2Leftmouse
"---
nmap <2-leftmouse> <CR>
"---
nmap <C-Space>a :scs find a <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-Space>c :scs find c <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-Space>d :scs find d <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-Space>e :scs find e <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-Space>f :scs find f <C-R>=expand("<cfile>")<CR><CR>
"---
nmap <C-Space>g :scs find g <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-Space>i :scs find i ^<C-R>=expand("<cfile>")<CR>$<CR>
"---
nmap <C-Space>s :scs find s <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-Space>t :scs find t <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>a :cs find a <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>c :cs find c <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>d :cs find d <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>e :cs find e <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>f :cs find f <C-R>=expand("<cfile>")<CR><CR>
"---
nmap <C-_>g :cs find g <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>i :cs find i ^<C-R>=expand("<cfile>")<CR>$<CR>
"---
nmap <C-_>s :cs find s <C-R>=expand("<cword>")<CR><CR>
"---
nmap <C-_>t :cs find t <C-R>=expand("<cword>")<CR><CR>
"---
nmap <Down> gj
"---
nmap <F1> <Plug>ManPreGetPage
"---
nmap <F9> :call <SID>SetFirstLine() <CR>
"---
nmap <LocalLeader>C <Plug>LUncomOff
"---
nmap <LocalLeader>S <Plug>OCamlSwitchNewWin
"---
nmap <LocalLeader>c <Plug>LUncomOn
"---
nmap <LocalLeader>h <Plug>ManBS
"---
nmap <LocalLeader>q <Plug>MailQuote
"---
nmap <LocalLeader>q <Plug>MailQuote;
"---
nmap <LocalLeader>s <Plug>OCamlSwitchEdit
"---
nmap <LocalLeader>t <Plug>OCamlPrintType
"---
nmap <SID>:  :<C-U>
"---
nmap <SID>c: :<C-U><C-R>=v:count ? v:count : ''<CR>
"---
nmap <Up>   gk
"---
nmap <c-h> <Plug>NetrwHideEdit
"---
nmap <c-l> <Plug>NetrwRefresh
"---
nmap <c-leftmouse>           <Plug>NetrwCLeftmouse
"---
nmap <c-leftmouse>         <Plug>NetrwCLeftmouse
"---
nmap <c-tab> <Plug>NetrwShrink
"---
nmap <leftmouse>             <Plug>NetrwLeftmouse
"---
nmap <leftmouse>           <Plug>NetrwLeftmouse
"---
nmap <middlemouse>                  <Plug>NetrwMiddlemouse
"---
nmap <middlemouse>           <Plug>NetrwMiddlemouse
"---
nmap <s-leftdrag>            <Plug>NetrwSLeftdrag
"---
nmap <s-leftdrag>          <Plug>NetrwSLeftdrag
"---
nmap <s-leftmouse>           <Plug>NetrwSLeftmouse
"---
nmap <s-leftmouse>         <Plug>NetrwSLeftmouse
"---
nmap <silent>              <Plug>Netrw2Leftmouse   -
"---
nmap <silent> %  <Plug>(MatchitNormalForward)
"---
nmap <silent> <2-leftmouse>       <Plug>NetrwReturn
"---
nmap <silent> <c-leftmouse>       <Plug>NetrwReturn
"---
nmap <silent> <nowait> <s-cr>                      <Plug>NetrwTreeSqueeze
"---
nmap <silent> <nowait> <s-cr>      <Plug>NetrwTreeSqueeze
"---
nmap <silent> <nowait> YOURKEYCOMBO  <Plug>NetrwTreeSqueeze
"---
nmap <silent> [% <Plug>(MatchitNormalMultiBackward)
"---
nmap <silent> ]% <Plug>(MatchitNormalMultiForward)
"---
nmap <silent> g% <Plug>(MatchitNormalBackward)
"---
nmap K <Plug>RacketDoc
"---
nmap _j :%call Justify('tw',4)<CR>
"---
nmap gC <Plug>(comment-toggle-end)
"---
nmap gc <Plug>(comment-toggle)
"---
nmap gcc <Plug>(comment-toggle-line)
"---
nmap gx <Plug>(open-word-under-cursor)
"---
nmap j      gj
"---
nmap k      gk
"---
nno  <silent>              <Plug>NetrwCLeftmouse   :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwCLeftmouse(0)<cr>
"---
nno  <silent>              <Plug>NetrwCLeftmouse   :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwCLeftmouse(1)<cr>
"---
nno  <silent>              <Plug>NetrwLeftmouse    :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwLeftmouse(0)<cr>
"---
nno  <silent>              <Plug>NetrwLeftmouse    :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwLeftmouse(1)<cr>
"---
nno  <silent>              <Plug>NetrwMiddlemouse  :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwPrevWinOpen(1)<cr>
"---
nno  <silent>              <Plug>NetrwSLeftdrag    :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwSLeftdrag(0)<cr>
"---
nno  <silent>              <Plug>NetrwSLeftdrag    :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwSLeftdrag(1)<cr>
"---
nno  <silent>              <Plug>NetrwSLeftmouse   :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwSLeftmouse(0)<cr>
"---
nno  <silent>              <Plug>NetrwSLeftmouse   :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwSLeftmouse(1)<cr>
"---
nno  <silent>              <middlemouse>           <Plug>NetrwMiddlemouse :exec "norm! \<lt>leftmouse>"<bar>call <SID>NetrwPrevWinOpen(0)<cr>
"---
nno <silent> <Plug>NetrwReturn       :Rexplore<cr>
"---
nno <silent> <Plug>NetrwShrink :call netrw#Shrink()<cr>
"---
nno <silent> <s-leftrelease> <leftmouse>:<c-u>call <SID>NetrwSLeftrelease(0)<cr>
"---
nno <silent> <s-leftrelease> <leftmouse>:<c-u>call <SID>NetrwSLeftrelease(1)<cr>
"---
nno <silent> KEYMAP-SEQUENCE
"---
nno <silent> [    :sil call <SID>TreeListMove('[')<cr>
"---
nno <silent> ]    :sil call <SID>TreeListMove(']')<cr>
"---
nnoremap <C-]>    :call ada#Jump_Tag ('', 'tjump')<cr>
"---
nnoremap <C-]>    :call ada#List_Tag ()<cr>
"---
nnoremap <F1>                       :he netrw-quickhelp<cr>
"---
nnoremap <F3> aText <Cmd>echo mode(1)<CR> Added<Esc>
"---
nnoremap <F4> <Cmd>let &opfunc='{t ->
"---
nnoremap <F4> <ScriptCmd>impl.DoTheWork()<CR>
"---
nnoremap <Leader>K :call dist#man#PreGetPage(0)<CR>
"---
nnoremap <Leader>hc <Plug>HelpCurwin;
"---
nnoremap <Leader>ht <Cmd>HelpToc<CR>
"---
nnoremap <Plug>(open-word-under-cursor) <scriptcmd>vim9.Open(GetWordUnderCursor())<CR>
"---
nnoremap <Plug>HelpCurwin; <ScriptCmd>helpcurwin.Open(expand('<cWORD>'))<CR>
"---
nnoremap <Plug>LUncomOff :s/^(\* \(.*\) \*)/\1/<CR>:noh<CR>
"---
nnoremap <Plug>LUncomOff <ESC>:s/^\/\* \(.*\) \*\//\1/<CR>
"---
nnoremap <Plug>LUncomOn gI(* <End> *)<ESC>
"---
nnoremap <Plug>LUncomOn mz0i/* <ESC>$A */<ESC>`z
"---
nnoremap <Plug>MailQuote :.,$s/^/> /<CR>:noh<CR>``
"---
nnoremap <Plug>MailQuote; :.,$s/^/> /<CR>
"---
nnoremap <Plug>ManPreGetPage :call dist#man#PreGetPage(0)<CR>
"---
nnoremap <Plug>OCamlSwitchEdit :<C-u>call OCaml_switch(0)<CR>
"---
nnoremap <Plug>OCamlSwitchNewWin :<C-u>call OCaml_switch(1)<CR>
"---
nnoremap <Plug>RacketDoc :call <SID>RacketDoc(expand('<cword>'))<CR>
"---
nnoremap <expr> <C-H> has("gui_running") ? ":promptrepl\<CR>" : "\<C-H>"
"---
nnoremap <expr> <F3> <SID>OpenPopup()
"---
nnoremap <expr> <F4> CountSpaces()
"---
nnoremap <expr> <F4><F4> CountSpaces() .. '_'
"---
nnoremap <expr> GG ":echom " .. screencol() .. "\n"
"---
nnoremap <expr> x StoreColumn()
"---
nnoremap <silent> < :set opfunc=<SID>DecreaseFunc<CR>g@
"---
nnoremap <silent> <2-LeftMouse> :call tutor#MouseDoubleClick()<cr>
"---
nnoremap <silent> << :call CobolIndentBlock(-1)<CR>
"---
nnoremap <silent> <C-A> :<C-U><C-R>=v:count1<CR>Cycle<CR>
"---
nnoremap <silent> <C-W><C-D>  :<C-U>exe <SID>jump('split',v:count)<SID>foldopen<CR>
"---
nnoremap <silent> <C-W>d      :<C-U>exe <SID>jump('split',v:count)<SID>foldopen<CR>
"---
nnoremap <silent> <C-X> :<C-U><C-R>=v:count1<CR>Cycle!<CR>
"---
nnoremap <silent> <CR> :call tutor#FollowLink(0)<cr>
"---
nnoremap <silent> <Leader>o :<C-u>call <SID>new_changelog_entry('')<CR>
"---
nnoremap <silent> <Leader>o :call <SID>open_changelog()<CR>
"---
nnoremap <silent> <Plug>(MatchitNormalBackward)    :<C-U>call matchit#Match_wrapper('',0,'n')<CR>
"---
nnoremap <silent> <Plug>(MatchitNormalForward)     :<C-U>call matchit#Match_wrapper('',1,'n')<CR>
"---
nnoremap <silent> <Plug>(MatchitNormalMultiBackward)    :<C-U>call matchit#MultiMatch("bW", "n")<CR>
"---
nnoremap <silent> <Plug>(MatchitNormalMultiForward)     :<C-U>call matchit#MultiMatch("W",  "n")<CR>
"---
nnoremap <silent> <Plug>ManBS :setl ma<Bar>%s/.\b//g
"---
nnoremap <silent> <Plug>NetrwBadd_cB                :<c-u>call <SID>NetrwBadd(0,1)<cr>
"---
nnoremap <silent> <Plug>NetrwBadd_cB                :<c-u>call <SID>NetrwBadd(1,1)<cr>
"---
nnoremap <silent> <Plug>NetrwBadd_cb                :<c-u>call <SID>NetrwBadd(0,0)<cr>
"---
nnoremap <silent> <Plug>NetrwBadd_cb                :<c-u>call <SID>NetrwBadd(1,0)<cr>
"---
nnoremap <silent> <Plug>NetrwBookHistHandler_gb     :<c-u>call <SID>NetrwBookHistHandler(1,b:netrw_curdir)<cr>
"---
nnoremap <silent> <Plug>NetrwBrowseUpDir            :<c-u>call <SID>NetrwBrowseUpDir(0)<cr>
"---
nnoremap <silent> <Plug>NetrwBrowseUpDir            :<c-u>call <SID>NetrwBrowseUpDir(1)<cr>
"---
nnoremap <silent> <Plug>NetrwHideEdit               :call <SID>NetrwHideEdit(1)<cr>
"---
nnoremap <silent> <Plug>NetrwHideEdit       :call <SID>NetrwHideEdit(0)<cr>
"---
nnoremap <silent> <Plug>NetrwHide_a                 :<c-u>call <SID>NetrwHide(0)<cr>
"---
nnoremap <silent> <Plug>NetrwHide_a                 :<c-u>call <SID>NetrwHide(1)<cr>
"---
nnoremap <silent> <Plug>NetrwLcd                    :<c-u>call <SID>NetrwLcd(b:netrw_curdir)<cr>
"---
nnoremap <silent> <Plug>NetrwLocalBrowseCheck       :<c-u>call <SID>NetrwBrowse(0,<SID>NetrwBrowseChgDir(0,<SID>NetrwGetWord(),1))<cr>
"---
nnoremap <silent> <Plug>NetrwLocalBrowseCheck       :<c-u>call netrw#LocalBrowseCheck(<SID>NetrwBrowseChgDir(1,<SID>NetrwGetWord(),1))<cr>
"---
nnoremap <silent> <Plug>NetrwMakeDir                :<c-u>call <SID>NetrwMakeDir("")<cr>
"---
nnoremap <silent> <Plug>NetrwOpenFile               :<c-u>call <SID>NetrwOpenFile(0)<cr>
"---
nnoremap <silent> <Plug>NetrwOpenFile               :<c-u>call <SID>NetrwOpenFile(1)<cr>
"---
nnoremap <silent> <Plug>NetrwRefresh                :<c-u>call <SID>NetrwRefresh(0,<SID>NetrwBrowseChgDir(0,'./',0))<cr>
"---
nnoremap <silent> <Plug>NetrwRefresh                <c-l>:call <SID>NetrwRefresh(1,<SID>NetrwBrowseChgDir(1,(exists("w:netrw_liststyle") && exists("w:netrw_treetop") && w:netrw_liststyle == 3)? w:netrw_treetop : './',0))<cr>
"---
nnoremap <silent> <Plug>NetrwRefresh        :call <SID>NetrwRefresh(0,<SID>NetrwBrowseChgDir(0,'./',0))<cr>
"---
nnoremap <silent> <Plug>NetrwServerEdit             :<c-u>call <SID>NetrwServerEdit(2,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <Plug>NetrwServerEdit             :<c-u>call <SID>NetrwServerEdit(3,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <Plug>NetrwSetChgwin              :<c-u>call <SID>NetrwSetChgwin()<cr>
"---
nnoremap <silent> <Plug>NetrwTreeSqueeze            :call <SID>TreeSqueezeDir(1)<cr>
"---
nnoremap <silent> <Plug>NetrwTreeSqueeze    :call <SID>TreeSqueezeDir(0)<cr>
"---
nnoremap <silent> <Plug>OCamlPrintType :<C-U>call Ocaml_print_type("normal")<CR>
"---
nnoremap <silent> <c-]> :call dist#man#PreGetPage(v:count)<CR>
"---
nnoremap <silent> <c-t> :call dist#man#PopPage()<CR>
"---
nnoremap <silent> <expr> <Plug>(comment-toggle) comment.Toggle()
"---
nnoremap <silent> <expr> <Plug>(comment-toggle-end) comment.Toggle() .. '$'
"---
nnoremap <silent> <expr> <Plug>(comment-toggle-line) comment.Toggle() .. '_'
"---
nnoremap <silent> <nowait> I        :<c-u>call <SID>NetrwBannerCtrl(1)<cr>
"---
nnoremap <silent> <nowait> O        :<c-u>call <SID>NetrwObtain(0)<cr>
"---
nnoremap <silent> <nowait> O        :<c-u>call <SID>NetrwObtain(1)<cr>
"---
nnoremap <silent> <nowait> P        :<c-u>call <SID>NetrwPrevWinOpen(0)<cr>
"---
nnoremap <silent> <nowait> P        :<c-u>call <SID>NetrwPrevWinOpen(1)<cr>
"---
nnoremap <silent> <nowait> S        :<c-u>call <SID>NetSortSequence(0)<cr>
"---
nnoremap <silent> <nowait> S        :<c-u>call <SID>NetSortSequence(1)<cr>
"---
nnoremap <silent> <nowait> Tb       :<c-u>call <SID>NetrwSetTgt(0,'b',v:count1)<cr>
"---
nnoremap <silent> <nowait> Tb       :<c-u>call <SID>NetrwSetTgt(1,'b',v:count1)<cr>
"---
nnoremap <silent> <nowait> Th       :<c-u>call <SID>NetrwSetTgt(0,'h',v:count)<cr>
"---
nnoremap <silent> <nowait> Th       :<c-u>call <SID>NetrwSetTgt(1,'h',v:count)<cr>
"---
nnoremap <silent> <nowait> U        :<c-u>call <SID>NetrwBookHistHandler(5,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> U        :<c-u>call <SID>NetrwBookHistHandler(5,expand("%"))<cr>
"---
nnoremap <silent> <nowait> X        :<c-u>call <SID>NetrwLocalExecute(expand("<cword>"))"<cr>
"---
nnoremap <silent> <nowait> gd       :<c-u>call <SID>NetrwForceChgDir(0,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> gd       :<c-u>call <SID>NetrwForceChgDir(1,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> gf       :<c-u>call <SID>NetrwForceFile(0,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> gf       :<c-u>call <SID>NetrwForceFile(1,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> gh       :<c-u>call <SID>NetrwHidden(0)<cr>
"---
nnoremap <silent> <nowait> gh       :<c-u>call <SID>NetrwHidden(1)<cr>
"---
nnoremap <silent> <nowait> gn       :<c-u>call netrw#SetTreetop(0,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> gp       :<c-u>call <SID>NetrwChgPerm(0,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> gp       :<c-u>call <SID>NetrwChgPerm(1,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> i        :<c-u>call <SID>NetrwListStyle(0)<cr>
"---
nnoremap <silent> <nowait> i        :<c-u>call <SID>NetrwListStyle(1)<cr>
"---
nnoremap <silent> <nowait> mA       :<c-u>call <SID>NetrwMarkFileArgList(0,1)<cr>
"---
nnoremap <silent> <nowait> mA       :<c-u>call <SID>NetrwMarkFileArgList(1,1)<cr>
"---
nnoremap <silent> <nowait> mB       :<c-u>call <SID>NetrwBookHistHandler(6,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> mF       :<c-u>call <SID>NetrwUnmarkList(bufnr("%"),b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> mT       :<c-u>call <SID>NetrwMarkFileTag(0)<cr>
"---
nnoremap <silent> <nowait> mT       :<c-u>call <SID>NetrwMarkFileTag(1)<cr>
"---
nnoremap <silent> <nowait> mX       :<c-u>call <SID>NetrwMarkFileExe(0,1)<cr>
"---
nnoremap <silent> <nowait> mX       :<c-u>call <SID>NetrwMarkFileExe(1,1)<cr>
"---
nnoremap <silent> <nowait> ma       :<c-u>call <SID>NetrwMarkFileArgList(0,0)<cr>
"---
nnoremap <silent> <nowait> ma       :<c-u>call <SID>NetrwMarkFileArgList(1,0)<cr>
"---
nnoremap <silent> <nowait> mb       :<c-u>call <SID>NetrwBookHistHandler(0,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> mc       :<c-u>call <SID>NetrwMarkFileCopy(0)<cr>
"---
nnoremap <silent> <nowait> mc       :<c-u>call <SID>NetrwMarkFileCopy(1)<cr>
"---
nnoremap <silent> <nowait> md       :<c-u>call <SID>NetrwMarkFileDiff(0)<cr>
"---
nnoremap <silent> <nowait> md       :<c-u>call <SID>NetrwMarkFileDiff(1)<cr>
"---
nnoremap <silent> <nowait> me       :<c-u>call <SID>NetrwMarkFileEdit(0)<cr>
"---
nnoremap <silent> <nowait> me       :<c-u>call <SID>NetrwMarkFileEdit(1)<cr>
"---
nnoremap <silent> <nowait> mf       :<c-u>call <SID>NetrwMarkFile(0,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> mf       :<c-u>call <SID>NetrwMarkFile(1,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> mg       :<c-u>call <SID>NetrwMarkFileGrep(0)<cr>
"---
nnoremap <silent> <nowait> mg       :<c-u>call <SID>NetrwMarkFileGrep(1)<cr>
"---
nnoremap <silent> <nowait> mh       :<c-u>call <SID>NetrwMarkHideSfx(0)<cr>
"---
nnoremap <silent> <nowait> mh       :<c-u>call <SID>NetrwMarkHideSfx(1)<cr>
"---
nnoremap <silent> <nowait> mm       :<c-u>call <SID>NetrwMarkFileMove(0)<cr>
"---
nnoremap <silent> <nowait> mm       :<c-u>call <SID>NetrwMarkFileMove(1)<cr>
"---
nnoremap <silent> <nowait> mr       :<c-u>call <SID>NetrwMarkFileRegexp(0)<cr>
"---
nnoremap <silent> <nowait> mr       :<c-u>call <SID>NetrwMarkFileRegexp(1)<cr>
"---
nnoremap <silent> <nowait> ms       :<c-u>call <SID>NetrwMarkFileSource(0)<cr>
"---
nnoremap <silent> <nowait> ms       :<c-u>call <SID>NetrwMarkFileSource(1)<cr>
"---
nnoremap <silent> <nowait> mt       :<c-u>call <SID>NetrwMarkFileTgt(0)<cr>
"---
nnoremap <silent> <nowait> mt       :<c-u>call <SID>NetrwMarkFileTgt(1)<cr>
"---
nnoremap <silent> <nowait> mu       :<c-u>call <SID>NetrwUnMarkFile(0)<cr>
"---
nnoremap <silent> <nowait> mu       :<c-u>call <SID>NetrwUnMarkFile(1)<cr>
"---
nnoremap <silent> <nowait> mv       :<c-u>call <SID>NetrwMarkFileVimCmd(0)<cr>
"---
nnoremap <silent> <nowait> mv       :<c-u>call <SID>NetrwMarkFileVimCmd(1)<cr>
"---
nnoremap <silent> <nowait> mx       :<c-u>call <SID>NetrwMarkFileExe(0,0)<cr>
"---
nnoremap <silent> <nowait> mx       :<c-u>call <SID>NetrwMarkFileExe(1,0)<cr>
"---
nnoremap <silent> <nowait> mz       :<c-u>call <SID>NetrwMarkFileCompress(0)<cr>
"---
nnoremap <silent> <nowait> mz       :<c-u>call <SID>NetrwMarkFileCompress(1)<cr>
"---
nnoremap <silent> <nowait> o        :call <SID>NetrwSplit(0)<cr>
"---
nnoremap <silent> <nowait> o        :call <SID>NetrwSplit(3)<cr>
"---
nnoremap <silent> <nowait> p        :<c-u>call <SID>NetrwPreview(<SID>NetrwBrowseChgDir(1,<SID>NetrwGetWord(),1,1))<cr>
"---
nnoremap <silent> <nowait> qF       :<c-u>call <SID>NetrwMarkFileQFEL(0,getqflist())<cr>
"---
nnoremap <silent> <nowait> qF       :<c-u>call <SID>NetrwMarkFileQFEL(1,getqflist())<cr>
"---
nnoremap <silent> <nowait> qL       :<c-u>call <SID>NetrwMarkFileQFEL(0,getloclist(v:count))<cr>
"---
nnoremap <silent> <nowait> qL       :<c-u>call <SID>NetrwMarkFileQFEL(1,getloclist(v:count))<cr>
"---
nnoremap <silent> <nowait> qb       :<c-u>call <SID>NetrwBookHistHandler(2,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> qf       :<c-u>call <SID>NetrwFileInfo(0,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> qf       :<c-u>call <SID>NetrwFileInfo(1,<SID>NetrwGetWord())<cr>
"---
nnoremap <silent> <nowait> r        :<c-u>let g:netrw_sort_direction= (g:netrw_sort_direction =~# 'n')? 'r' : 'n'<bar>exe "norm! 0"<bar>call <SID>NetrwBrowse(0,<SID>NetrwBrowseChgDir(0,'./',0))<cr>
"---
nnoremap <silent> <nowait> r        :<c-u>let g:netrw_sort_direction= (g:netrw_sort_direction =~# 'n')? 'r' : 'n'<bar>exe "norm! 0"<bar>call <SID>NetrwRefresh(1,<SID>NetrwBrowseChgDir(1,'./',0))<cr>
"---
nnoremap <silent> <nowait> s        :call <SID>NetrwSortStyle(0)<cr>
"---
nnoremap <silent> <nowait> s        :call <SID>NetrwSortStyle(1)<cr>
"---
nnoremap <silent> <nowait> t        :call <SID>NetrwSplit(1)<cr>
"---
nnoremap <silent> <nowait> t        :call <SID>NetrwSplit(4)<cr>
"---
nnoremap <silent> <nowait> u        :<c-u>call <SID>NetrwBookHistHandler(4,b:netrw_curdir)<cr>
"---
nnoremap <silent> <nowait> u        :<c-u>call <SID>NetrwBookHistHandler(4,expand("%"))<cr>
"---
nnoremap <silent> <nowait> v        :call <SID>NetrwSplit(2)<cr>
"---
nnoremap <silent> <nowait> v        :call <SID>NetrwSplit(5)<cr>
"---
nnoremap <silent> <nowait> x        :<c-u>call netrw#BrowseX(<SID>NetrwBrowseChgDir(1,<SID>NetrwGetWord(),1,0))"<cr>
"---
nnoremap <silent> <s-down> :Nexplore<cr>
"---
nnoremap <silent> <s-up>   :Pexplore<cr>
"---
nnoremap <silent> > :set opfunc=<SID>IncreaseFunc<CR>g@
"---
nnoremap <silent> >> :call CobolIndentBlock(1)<CR>
"---
nnoremap <silent> GG :echom screencol()<CR>
"---
nnoremap <silent> [- :<C-U>call <SID>DoMotion(comment_block_end, 1, 'Wb')<CR>
"---
nnoremap <silent> [<C-D>      :<C-U>exe <SID>jump('edit',v:count)<SID>foldopen<CR>
"---
nnoremap <silent> [M :<C-U>call <SID>searchsyn('\<end\>',['rubyDefine'],'b','n')<CR>
"---
nnoremap <silent> [[       :sil call <SID>TreeListMove('[[')<cr>
"---
nnoremap <silent> [[ :<C-U>call <SID>DoMotion(sections, v:count1, 'Wb')<CR>
"---
nnoremap <silent> [[ :<C-U>call <SID>searchsyn('\<\%(class\<Bar>module\)\>',['rubyModule','rubyClass'],'b','n')<CR>
"---
nnoremap <silent> [[ :call rust#Jump('n', 'Back')<CR>
"---
nnoremap <silent> [[ :call search('\c^\s*begin\>', 'bW' )<CR>
"---
nnoremap <silent> [[ <Cmd>call <SID>VbSearch('^\s*\%(\%(private\<Bar>public\)\s\+\)\=\%(function\<Bar>sub\)', 'sbW')<CR>
"---
nnoremap <silent> [[ m`:call ZimbuGoStartBlock()<CR>
"---
nnoremap <silent> []       :sil call <SID>TreeListMove('[]')<cr>
"---
nnoremap <silent> [] :<C-U>call <SID>searchsyn('\<end\>',['rubyModule','rubyClass'],'b','n')<CR>
"---
nnoremap <silent> [] :call search('\c^\s*end\W*$', 'bW' )<CR>
"---
nnoremap <silent> [] <Cmd>call <SID>VbSearch('^\s*end\s\+\%(function\<Bar>sub\)', 'sbW')<CR>
"---
nnoremap <silent> [d          :<C-U>exe <SID>jump('pedit',v:count)<CR>
"---
nnoremap <silent> [m :<C-U>call <SID>DoFeatureMotion(v:count1, 'Wb')<CR>
"---
nnoremap <silent> [m :<C-U>call <SID>searchsyn('\<def\>',['rubyDefine'],'b','n')<CR>
"---
nnoremap <silent> ]- :<C-U>call <SID>DoMotion(comment_block_start, 1, 'W')<CR>
"---
nnoremap <silent> ]<C-D>      :<C-U>exe <SID>jump('edit',v:count)<SID>foldopen<CR>
"---
nnoremap <silent> ]M :<C-U>call <SID>searchsyn('\<end\>',['rubyDefine'],'','n')<CR>
"---
nnoremap <silent> ][       :sil call <SID>TreeListMove('][')<cr>
"---
nnoremap <silent> ][ :<C-U>call <SID>searchsyn('\<end\>',['rubyModule','rubyClass'],'','n')<CR>
"---
nnoremap <silent> ][ :call search('\c^\s*end\W*$', 'W' )<CR>
"---
nnoremap <silent> ][ <Cmd>call <SID>VbSearch('^\s*end\s\+\%(function\<Bar>sub\)', 'sW')<CR>
"---
nnoremap <silent> ]]       :sil call <SID>TreeListMove(']]')<cr>
"---
nnoremap <silent> ]] :<C-U>call <SID>DoMotion(sections, v:count1, 'W')<CR>
"---
nnoremap <silent> ]] :<C-U>call <SID>searchsyn('\<\%(class\<Bar>module\)\>',['rubyModule','rubyClass'],'','n')<CR>
"---
nnoremap <silent> ]] :call rust#Jump('n', 'Forward')<CR>
"---
nnoremap <silent> ]] :call search('\c^\s*begin\>', 'W' )<CR>
"---
nnoremap <silent> ]] <Cmd>call <SID>VbSearch('^\s*\%(\%(private\<Bar>public\)\s\+\)\=\%(function\<Bar>sub\)', 'sW')<CR>
"---
nnoremap <silent> ]] m`:call ZimbuGoEndBlock()<CR>
"---
nnoremap <silent> ]d          :<C-U>exe <SID>jump('pedit',v:count)<CR>
"---
nnoremap <silent> ]m :<C-U>call <SID>DoFeatureMotion(v:count1, 'W')<CR>
"---
nnoremap <silent> ]m :<C-U>call <SID>searchsyn('\<def\>',['rubyDefine'],'','n')<CR>
"---
nnoremap <silent><C-T> :if len(b:pdf_tagstack) > 0 <Bar> call setpos('.',remove(b:pdf_tagstack, -1)) <Bar> else <Bar> exe "norm! \<Lt>C-T>" <Bar> endif<CR>
"---
nnoremap <silent><C-W>f :<C-U>call vimgoto#Find("\<lt>C-W>F")<CR>
"---
nnoremap <silent><C-W>gf :<C-U>call vimgoto#Find("\<lt>C-W>gF")<CR>
"---
nnoremap <silent><C-]> :call <SID>Tag()<CR>
"---
nnoremap <silent><leader>t <scriptcmd>MyConTeXt()<cr>
"---
nnoremap <silent>[" :call search('\%(^\s*".*\n\)\%(^\s*"\)\@!', "bW")<CR>
"---
nnoremap <silent>[[ :<C-U>call search('\%(^#\{1,5\}\s\+\S\\|^\S.*\n^[=-]\+$\)', "bsW")<CR>
"---
nnoremap <silent>[[ :call <SID>Abaqus_NextKeyword(-1)<CR>
"---
nnoremap <silent>[[ <scriptcmd>MoveAround(v:count1, "beginsection", "bW")<cr>
"---
nnoremap <silent>[[ <scriptcmd>NextSection(true, v:count1)<CR>
"---
nnoremap <silent>[[ m':call search('^\s*\(fu\%[nction]\\|\(export\s\+\)\?def\)\>', "bW")<CR>
"---
nnoremap <silent>[] <scriptcmd>MoveAround(v:count1, "endsection",   "bW")<cr>
"---
nnoremap <silent>[] m':call search('^\s*end\(f\%[unction]\\|\(export\s\+\)\?def\)\>', "bW")<CR>
"---
nnoremap <silent>[{ <scriptcmd>MoveAround(v:count1, "beginblock",   "bW")<cr>
"---
nnoremap <silent>]" :call search('\%(^\s*".*\n\)\@<!\%(^\s*"\)', "W")<CR>
"---
nnoremap <silent>][ <scriptcmd>MoveAround(v:count1, "endsection",   "W") <cr>
"---
nnoremap <silent>][ m':call search('^\s*end\(f\%[unction]\\|\(export\s\+\)\?def\)\>', "W")<CR>
"---
nnoremap <silent>]] :<C-U>call search('\%(^#\{1,5\}\s\+\S\\|^\S.*\n^[=-]\+$\)', "sW")<CR>
"---
nnoremap <silent>]] :call <SID>Abaqus_NextKeyword(1)<CR>
"---
nnoremap <silent>]] <scriptcmd>MoveAround(v:count1, "beginsection", "W") <cr>
"---
nnoremap <silent>]] <scriptcmd>NextSection(false, v:count1)<CR>
"---
nnoremap <silent>]] m':call search('^\s*\(fu\%[nction]\\|\(export\s\+\)\?def\)\>', "W")<CR>
"---
nnoremap <silent>]} <scriptcmd>MoveAround(v:count1, "endblock",     "W") <cr>
"---
nnoremap <silent>gf :<C-U>call vimgoto#Find('gF')<CR>
"---
nnoremap >> :call tutor#InjectCommand()<cr>
"---
nnoremap GG <Cmd>echom screencol()<CR>
"---
nnoremap K :Evaluate<CR>
"---
nnoremap K somethingelse
"---
nnoremap g<C-]>   :call ada#Jump_Tag ('','stjump')<cr>
"---
nnoremap g<C-]>   :call ada#List_Tag ()<cr>
"---
nnoremap z/  :exe ':spellrare! ' .. expand('<cWORD>')<CR>
"---
nnoremap z?  :exe ':spellrare  ' .. expand('<cWORD>')<CR>
"---
no		no	no     on	  x	    fail
"---
no		no	x      off	  x	    read other file, goto tag
"---
no		yes	no     off	  off	    fail
"---
no		yes	no     off	  on	    write current file,
"---
no		yes	no     on	  off	    fail
"---
no		yes	no     on	  on	    fail
"---
no		yes	no     on	  x	    fail
"---
no		yes	yes    off	  x	    abandon current file,
"---
no	Don't use ALT keys for menus.  ALT key combinations can be
"---
no     no      when buffer created
"---
no "then" after the "if", the region doesn't match.
"---
no %		The buffer list will not be saved nor read back.
"---
no /		Since '/' is not specified, the default will be used,
"---
no SwapExists autocommand.  The default is empty.
"---
no [vimhome] is specified.  Otherwise, the [vimhome] argument allows
"---
no actual character.  This can be halfway into a tab or beyond the end
"---
no argument has been specified.
"---
no character is available.
"---
no clipboard support, you will see this warning:
"---
no count, the "-s" is removed.
"---
no effect on deeper levels of nesting.  Affects flags like "wN"
"---
no effect.  You need to quit Vim and start it again. (Rationale: the items
"---
no error message is given.
"---
no error message.
"---
no error message. (Ugen)
"---
no faster than computers are growing.  Keep Vim usable on older systems.
"---
no function calls, to avoid a security problem.
"---
no h		'hlsearch' highlighting will be restored.
"---
no identifier.  They are mostly referred to by the number of the change above
"---
no item is selected when using the <Up> or
"---
no items act like empty strings.
"---
no language mappings are used, the other when they
"---
no lines are checked.  See |modeline|.
"---
no longer exist.  You only need to set 'guifont' and everything should
"---
no longer exists" messages.  (Moore)
"---
no longer hidden.
"---
no longer necessary as Patch 8.2.3661 has changed the count to
"---
no longer selected.
"---
no matching item in the completion menu
"---
no matter how many times items from it are imported.
"---
no matter what 'encoding' is set to.  Write the message in
"---
no max line indicator.
"---
no need to call "super()" or "new()" on the parent.
"---
no need to use IME, even when editing multibyte text.  So when exiting Insert
"---
no pending input (zeertzjq).
"---
no previous search pattern.
"---
no prompt is set with |prompt_setprompt()|, "%
"---
no recognized errors, close the window.
"---
no restrictions on applications.
"---
no save messages were displayed after a save.
"---
no sequence or other interfaces.
"---
no space above/below the cursor then show the popup to
"---
no specialized variable has a corresponding empty value.
"---
no swap file will be used when xchacha20 encryption is
"---
no tabs.  In addition, any new file must include a modeline with `set et` to
"---
no text selected in the second line:
"---
no uppercase characters, the added character is converted to
"---
no valid line at all).  You can use ":cl!" to display all the error messages.
"---
no valuable existing files are overwritten.
"---
no way to see the command at the current line yet.
"---
no window will be used.
"---
noremap	 <2-LeftMouse>	<2-RightMouse>
"---
noremap	 <2-RightMouse>	<2-LeftMouse>
"---
noremap	 <3-LeftMouse>	<3-RightMouse>
"---
noremap	 <3-RightMouse>	<3-LeftMouse>
"---
noremap	 <4-LeftMouse>	<4-RightMouse>
"---
noremap	 <4-RightMouse>	<4-LeftMouse>
"---
noremap	 <LeftDrag>	<RightDrag>
"---
noremap	 <LeftMouse>	<RightMouse>
"---
noremap	 <LeftRelease>	<RightRelease>
"---
noremap	 <RightDrag>	<LeftDrag>
"---
noremap	 <RightMouse>	<LeftMouse>
"---
noremap	 <RightRelease>	<LeftRelease>
"---
noremap	 g<LeftMouse>	<C-RightMouse>
"---
noremap	 g<RightMouse>	<C-LeftMouse>
"---
noremap  <F8> :let &iminsert = ! &iminsert<CR>
"---
noremap  <expr> <C-F> has("gui_running") ? ":promptfind\<CR>" : "/"
"---
noremap  <silent>[[ :<C-u>cal <SID>CountWrapper(':cal search("\\%(--.*\\)\\@<!\\%(\\<end\\s\\+\\)\\@<!\\<".b:vhdl_explore."\\>\\c\\<Bar>\\%^","bW")')<CR>
"---
noremap  <silent>[] :<C-u>cal <SID>CountWrapper(':cal search("\\%(--.*\\)\\@<!\\<end\\s\\+".b:vhdl_explore."\\>\\c\\<Bar>\\%^","bW")')<CR>
"---
noremap  <silent>][ :<C-u>cal <SID>CountWrapper(':cal search("\\%(--.*\\)\\@<!\\<end\\s\\+".b:vhdl_explore."\\>\\c\\<Bar>\\%$","W")')<CR>
"---
noremap  <silent>]] :<C-u>cal <SID>CountWrapper(':cal search("\\%(--.*\\)\\@<!\\%(\\<end\\s\\+\\)\\@<!\\<".b:vhdl_explore."\\>\\c\\<Bar>\\%$","W")')<CR>
"---
noremap  [% <Nop>
"---
noremap  ]% <Nop>
"---
noremap % %<SID>L
"---
noremap ,a <ScriptCmd>:call s:that.OtherFunc()<CR>
"---
noremap ,a <ScriptCmd>name.Function()<CR>
"---
noremap / H$:call <SID>Forward()<CR>/
"---
noremap <C-A> gggH<C-O>G
"---
noremap <C-F4> <C-W>c
"---
noremap <C-Q>		<C-V>
"---
noremap <C-R> <C-L><SID>L
"---
noremap <C-S>		:update<CR>
"---
noremap <C-Tab> <C-W>w
"---
noremap <C-U> <C-U><SID>L
"---
noremap <C-V>      gP
"---
noremap <C-Y> <C-R>
"---
noremap <C-Z> u
"---
noremap <CR> <C-E><SID>L
"---
noremap <LocalLeader>c o/**************<CR><CR>/<Esc>
"---
noremap <M-Space> :simalt ~<CR>
"---
noremap <Plug>JavaImport; oimport ""<Left><Esc>
"---
noremap <Plug>SpecChangelog :call <SID>SpecChangelog("")<CR>
"---
noremap <S-Insert> gP
"---
noremap <SID>L L0:redraw<CR>:file<CR>
"---
noremap <SID>L Lg0:redraw<CR>:file<CR>
"---
noremap <Space> :call <SID>NextPage()<CR><SID>L
"---
noremap <silent> ,a :call <SID>name.Function()<CR>
"---
noremap <silent> <C-F> :promptfind<CR>
"---
noremap <silent> <CR> <C-\><C-N>:call <SID>CR()<CR>
"---
noremap <silent> <Plug>Comment          :call <SID>Comment('c')<CR>
"---
noremap <silent> <Plug>DeComment        :call <SID>Comment('u')<CR>
"---
noremap <silent> <Plug>Traditional      :call <SID>Comment('t')<CR>
"---
noremap <silent> <Plug>VisualComment     :'<,'>call <SID>Comment('c')<CR>
"---
noremap <silent> <Plug>VisualDeComment   :'<,'>call <SID>Comment('u')<CR>
"---
noremap <silent> <Plug>VisualTraditional :'<,'>call <SID>Comment('t')<CR>
"---
noremap <silent> <Space> :call <SID>Space()<CR>
"---
noremap <silent> <cr>		:call <SID>TarBrowseSelect()<cr>
"---
noremap <silent> <cr>		:call <SID>ZipBrowseSelect()<cr>
"---
noremap <silent> <leftmouse>	<leftmouse>:call <SID>TarBrowseSelect()<cr>
"---
noremap <silent> <leftmouse>	<leftmouse>:call <SID>ZipBrowseSelect()<cr>
"---
noremap <silent> [[
"---
noremap <silent> [[ :<C-U>call <SID>SearchSection(0, 1, '')<CR>
"---
noremap <silent> [[ <Cmd>call <SID>GoFindSection('prev_start', v:count1)<CR>
"---
noremap <silent> [[ <nop>
"---
noremap <silent> [[ m':call search('\c^\%(\s*\<Bar>.\{6\}\s\+\)\zs[A-Za-z0-9-]\+\s\+\%(division\<Bar>section\)\s*\.','bW')<CR>
"---
noremap <silent> []
"---
noremap <silent> [] :<C-U>call <SID>SearchSection(1, 1, '')<CR>
"---
noremap <silent> [] <Cmd>call <SID>GoFindSection('prev_end', v:count1)<CR>
"---
noremap <silent> [] <nop>
"---
noremap <silent> [] m':call <SID>toend('b')<CR>
"---
noremap <silent> ][
"---
noremap <silent> ][ :<C-U>call <SID>SearchSection(1, 0, '')<CR>
"---
noremap <silent> ][ <Cmd>call <SID>GoFindSection('next_end', v:count1)<CR>
"---
noremap <silent> ][ <nop>
"---
noremap <silent> ][ m':call <SID>toend('')<CR>
"---
noremap <silent> ]]
"---
noremap <silent> ]] :<C-U>call <SID>SearchSection(0, 0, '')<CR>
"---
noremap <silent> ]] <Cmd>call <SID>GoFindSection('next_start', v:count1)<CR>
"---
noremap <silent> ]] <nop>
"---
noremap <silent> ]] m':call search('\c^\%(\s*\<Bar>.\{6\}\s\+\)\zs[A-Za-z0-9-]\+\s\+\%(division\<Bar>section\)\.','W')<CR>
"---
noremap <silent> x		:call zip#Extract()<cr>
"---
noremap <silent> x	 	:call tar#Extract()<cr>
"---
noremap <silent><LocalLeader><LocalLeader>
"---
noremap <silent><LocalLeader>s :call <SID>Abaqus_Swap()<CR>
"---
noremap <silent>[# :call search('^\s*#\@!', "bW")<CR>
"---
noremap <silent>[[ :call search('^\s*sub\>', "bW")<CR>
"---
noremap <silent>[] :call search('^\s*endsub\>', "bW")<CR>
"---
noremap <silent>]# :call search('^\s*#\@!', "W")<CR>
"---
noremap <silent>][ :call search('^\s*endsub\>', "W")<CR>
"---
noremap <silent>]] :call search('^\s*sub\>', "W")<CR>
"---
noremap ? H0:call <SID>Backward()<CR>?
"---
noremap ? Hg0:call <SID>Backward()<CR>?
"---
noremap G G<SID>L
"---
noremap N H$Nzt<SID>L
"---
noremap N H0Nzt<SID>L
"---
noremap N Hg0Nzt<SID>L
"---
noremap Only remap mappings defined in this script that start
"---
noremap R <C-L><SID>L
"---
noremap Use only mappings local to the script, not global
"---
noremap b <C-B><SID>L
"---
noremap d <C-D><SID>L
"---
noremap g gg<SID>L
"---
noremap h :call <SID>Help()<CR>
"---
noremap k <C-Y><SID>L
"---
noremap n H$nzt<SID>L
"---
noremap n H0nzt<SID>L
"---
noremap n Hg0nzt<SID>L
"---
noremap q :q<CR>
"---
noremap r <C-L><SID>L
"---
noremap u <C-U><SID>L
"---
noremap x <Cmd>echo mode(1)<CR>
"---
noremap! <F8> <C-^>
"---
noremap! <LeftDrag>	<RightDrag>
"---
noremap! <LeftMouse>	<RightMouse>
"---
noremap! <LeftRelease>	<RightRelease>
"---
noremap! <RightDrag>	<LeftDrag>
"---
noremap! <RightMouse>	<LeftMouse>
"---
noremap! <RightRelease>	<LeftRelease>
"---
omap <silent> %  <Plug>(MatchitOperationForward)
"---
omap <silent> [% <Plug>(MatchitOperationMultiBackward)
"---
omap <silent> ]% <Plug>(MatchitOperationMultiForward)
"---
omap <silent> g% <Plug>(MatchitOperationBackward)
"---
omap ac <Plug>(comment-text-object-outer)
"---
omap ic <Plug>(comment-text-object-inner)
"---
omap xyzzy <Nop>
"---
onoremap <C-A> <C-C>gggH<C-O>G
"---
onoremap <C-F4> <C-C><C-W>c
"---
onoremap <C-Tab> <C-C><C-W>w
"---
onoremap <silent> <C-F> <C-C>:promptfind<CR>
"---
onoremap <silent> <Plug>(MatchitOperationBackward) :<C-U>call matchit#Match_wrapper('',0,'o')<CR>
"---
onoremap <silent> <Plug>(MatchitOperationForward)  :<C-U>call matchit#Match_wrapper('',1,'o')<CR>
"---
onoremap <silent> <Plug>(MatchitOperationMultiBackward) :<C-U>call matchit#MultiMatch("bW", "o")<CR>
"---
onoremap <silent> <Plug>(MatchitOperationMultiForward)  :<C-U>call matchit#MultiMatch("W",  "o")<CR>
"---
onoremap <silent> <Plug>(comment-text-object-inner) <scriptcmd>comment.ObjComment(v:true)<CR>
"---
onoremap <silent> <Plug>(comment-text-object-outer) <scriptcmd>comment.ObjComment(v:false)<CR>
"---
onoremap <silent> F :<C-U>normal! 0f(hviw<CR>
"---
onoremap <silent> [[ :call rust#Jump('o', 'Back')<CR>
"---
onoremap <silent> ]] :call rust#Jump('o', 'Forward')<CR>
"---
onoremap <silent> aM :<C-U>call <SID>wrap_a('[[','][')<CR>
"---
onoremap <silent> am :<C-U>call <SID>wrap_a('[m',']M')<CR>
"---
onoremap <silent> iM :<C-U>call <SID>wrap_i('[[','][')<CR>
"---
onoremap <silent> im :<C-U>call <SID>wrap_i('[m',']M')<CR>
"---
onoremap <silent>a$ <scriptcmd>normal! F$vf$<cr>
"---
onoremap <silent>i$ <scriptcmd>normal! T$vt$<cr>
"---
onoremap <silent>tp <scriptcmd>TeXPar()<cr>
"---
snoremap <C-A> <C-C>gggH<C-O>G
"---
tmap <expr> <Esc>]b SendToTerm("\<Esc>]b")
"---
tnoremap <C-t><C-t> <Cmd>HelpToc<CR>
"---
tnoremap <Esc> <C-W>N
"---
tnoremap <F1> <C-W>N
"---
vmap ,gq :s/\s\+/ /g<CR>gvgq
"---
vmap <Down> gj
"---
vmap <LocalLeader>C <Plug>BUncomOff
"---
vmap <LocalLeader>c <Plug>BUncomOn
"---
vmap <LocalLeader>q <Plug>MailQuote
"---
vmap <LocalLeader>q <Plug>MailQuote;
"---
vmap <S-Insert>		<C-V>
"---
vmap <Up>   gk
"---
vmap _j :call Justify('tw',4)<CR>
"---
vmap j      gj
"---
vmap k      gk
"---
vnoremap <BS> d
"---
vnoremap <C-C>
"---
vnoremap <C-C>      y
"---
vnoremap <C-Insert>
"---
vnoremap <C-Insert> y
"---
vnoremap <C-S>		<C-C>:update<CR>
"---
vnoremap <C-X>
"---
vnoremap <C-X>   x
"---
vnoremap <Plug>BUncomOff <ESC>:'<,'><CR>`<dd`>dd`<
"---
vnoremap <Plug>BUncomOn <ESC>:'<,'><CR>`<O<ESC>0i/*<ESC>`>o<ESC>0i*/<ESC>`<
"---
vnoremap <Plug>MailQuote :s/^/> /<CR>:noh<CR>``
"---
vnoremap <Plug>MailQuote; :s/^/> /<CR>
"---
vnoremap <S-Del>
"---
vnoremap <S-Del> x
"---
vnoremap <silent> < :call CobolIndentBlock(-v:count1)<CR>
"---
vnoremap <silent> <C-F> y:promptfind <C-R>"<CR>
"---
vnoremap <silent> > :call CobolIndentBlock(v:count1)<CR>
"---
vnoremap <silent> [[
"---
vnoremap <silent> [[ <Cmd>call <SID>VbSearch('^\s*\%(\%(private\<Bar>public\)\s\+\)\=\%(function\<Bar>sub\)', 'sbW')<CR>
"---
vnoremap <silent> []
"---
vnoremap <silent> [] <Cmd>call <SID>VbSearch('^\s*end\s\+\%(function\<Bar>sub\)', 'sbW')<CR>
"---
vnoremap <silent> ][
"---
vnoremap <silent> ][ <Cmd>call <SID>VbSearch('^\s*end\s\+\%(function\<Bar>sub\)', 'sW')<CR>
"---
vnoremap <silent> ]]
"---
vnoremap <silent> ]] <Cmd>call <SID>VbSearch('^\s*\%(\%(private\<Bar>public\)\s\+\)\=\%(function\<Bar>sub\)', 'sW')<CR>
"---
vnoremap <silent>[[ :<C-u>cal <SID>CountWrapper('[[')<CR>
"---
vnoremap <silent>[[ <scriptcmd>MoveAround(v:count1, "beginsection", "bW")<cr>
"---
vnoremap <silent>[] :<C-u>cal <SID>CountWrapper('[]')<CR>
"---
vnoremap <silent>[] <scriptcmd>MoveAround(v:count1, "endsection",   "bW")<cr>
"---
vnoremap <silent>[{ <scriptcmd>MoveAround(v:count1, "beginblock",   "bW")<cr>
"---
vnoremap <silent>][ :<C-u>cal <SID>CountWrapper('][')<CR>
"---
vnoremap <silent>][ <scriptcmd>MoveAround(v:count1, "endsection",   "W") <cr>
"---
vnoremap <silent>]] :<C-u>cal <SID>CountWrapper(']]')<CR>
"---
vnoremap <silent>]] <scriptcmd>MoveAround(v:count1, "beginsection", "W") <cr>
"---
vnoremap <silent>]} <scriptcmd>MoveAround(v:count1, "endblock",     "W") <cr>
"---
vnoremap <silent>tp <scriptcmd>TeXPar()<cr>
"---
vnoremap _g y:exe "grep /" .. escape(@", '\\/') .. "/ *.c *.h"<CR>
"---
vnoremap a$ F$of$
"---
vnoremap a% <Nop>
"---
vnoremap i$ T$ot$
"---
xmap <LocalLeader>C <Plug>BUncomOff
"---
xmap <LocalLeader>c <Plug>BUncomOn
"---
xmap <LocalLeader>t <Plug>OCamlPrintType
"---
xmap <silent> %  <Plug>(MatchitVisualForward)
"---
xmap <silent> <Plug>(MatchitVisualTextObject) <Plug>(MatchitVisualMultiBackward)o<Plug>(MatchitVisualMultiForward)
"---
xmap <silent> [% <Plug>(MatchitVisualMultiBackward)
"---
xmap <silent> ]% <Plug>(MatchitVisualMultiForward)
"---
xmap <silent> g% <Plug>(MatchitVisualBackward)
"---
xmap K <Plug>RacketDoc
"---
xmap a% <Plug>(MatchitVisualTextObject)
"---
xmap ac <Plug>(comment-text-object-outer)
"---
xmap gc <Plug>(comment-toggle)
"---
xmap gx <Plug>(open-word-under-cursor)
"---
xmap ic <Plug>(comment-text-object-inner)
"---
xnoremap <C-A> <C-C>ggVG
"---
xnoremap <Plug>(open-word-under-cursor) <scriptcmd>vim9.Open(getregion(getpos('v'), getpos('.'), { type: mode() })->join())<CR>
"---
xnoremap <Plug>BUncomOff <ESC>:'<,'><CR>`<dd`>dd`<
"---
xnoremap <Plug>BUncomOn <ESC>:'<,'><CR>`<O<ESC>0i(*<ESC>`>o<ESC>0i*)<ESC>`<
"---
xnoremap <Plug>RacketDoc :call <SID>Racket_visual_doc()<cr>
"---
xnoremap <expr> <F4> CountSpaces()
"---
xnoremap <silent> <Leader>o :<C-u>call <SID>new_changelog_entry('')<CR>
"---
xnoremap <silent> <Plug>(MatchitVisualBackward)    :<C-U>call matchit#Match_wrapper('',0,'v')<CR>m'gv``
"---
xnoremap <silent> <Plug>(MatchitVisualForward)     :<C-U>call matchit#Match_wrapper('',1,'v')<CR>
"---
xnoremap <silent> <Plug>(MatchitVisualMultiBackward)    :<C-U>call matchit#MultiMatch("bW", "n")<CR>m'gv``
"---
xnoremap <silent> <Plug>(MatchitVisualMultiForward)     :<C-U>call matchit#MultiMatch("W",  "n")<CR>m'gv``
"---
xnoremap <silent> <Plug>(comment-text-object-inner) <esc><scriptcmd>comment.ObjComment(v:true)<CR>
"---
xnoremap <silent> <Plug>(comment-text-object-outer) <esc><scriptcmd>comment.ObjComment(v:false)<CR>
"---
xnoremap <silent> <Plug>OCamlPrintType :<C-U>call Ocaml_print_type("visual")<CR>`<
"---
xnoremap <silent> <expr> <Plug>(comment-toggle) comment.Toggle()
"---
xnoremap <silent> [- :<C-U>exe "normal! gv"<Bar>call <SID>DoMotion(comment_block_end, 1, 'Wb')<CR>
"---
xnoremap <silent> [M :<C-U>call <SID>searchsyn('\<end\>',['rubyDefine'],'b','v')<CR>
"---
xnoremap <silent> [[ :<C-U>call <SID>SearchSection(0, 1, visualmode())<CR>
"---
xnoremap <silent> [[ :<C-U>call <SID>searchsyn('\<\%(class\<Bar>module\)\>',['rubyModule','rubyClass'],'b','v')<CR>
"---
xnoremap <silent> [[ :<C-U>exe "normal! gv"<Bar>call <SID>DoMotion(sections, v:count1, 'Wb')<CR>
"---
xnoremap <silent> [[ :<C-U>exec "normal! gv"<Bar>call search('\c^\s*begin\>', 'bW' )<CR>
"---
xnoremap <silent> [[ :call rust#Jump('v', 'Back')<CR>
"---
xnoremap <silent> [] :<C-U>call <SID>SearchSection(1, 1, visualmode())<CR>
"---
xnoremap <silent> [] :<C-U>call <SID>searchsyn('\<end\>',['rubyModule','rubyClass'],'b','v')<CR>
"---
xnoremap <silent> [] :<C-U>exec "normal! gv"<Bar>call search('\c^\s*end\W*$', 'bW' )<CR>
"---
xnoremap <silent> [m :<C-U>call <SID>searchsyn('\<def\>',['rubyDefine'],'b','v')<CR>
"---
xnoremap <silent> [m :<C-U>exe "normal! gv"<Bar>call <SID>DoFeatureMotion(v:count1, 'Wb')<CR>
"---
xnoremap <silent> ]- :<C-U>exe "normal! gv"<Bar>call <SID>DoMotion(comment_block_start, 1, 'W')<CR>
"---
xnoremap <silent> ]M :<C-U>call <SID>searchsyn('\<end\>',['rubyDefine'],'','v')<CR>
"---
xnoremap <silent> ][ :<C-U>call <SID>SearchSection(1, 0, visualmode())<CR>
"---
xnoremap <silent> ][ :<C-U>call <SID>searchsyn('\<end\>',['rubyModule','rubyClass'],'','v')<CR>
"---
xnoremap <silent> ][ :<C-U>exec "normal! gv"<Bar>call search('\c^\s*end\W*$', 'W' )<CR>
"---
xnoremap <silent> ]] :<C-U>call <SID>SearchSection(0, 0, visualmode())<CR>
"---
xnoremap <silent> ]] :<C-U>call <SID>searchsyn('\<\%(class\<Bar>module\)\>',['rubyModule','rubyClass'],'','v')<CR>
"---
xnoremap <silent> ]] :<C-U>exe "normal! gv"<Bar>call <SID>DoMotion(sections, v:count1, 'W')<CR>
"---
xnoremap <silent> ]] :<C-U>exec "normal! gv"<Bar>call search('\c^\s*begin\>', 'W' )<CR>
"---
xnoremap <silent> ]] :call rust#Jump('v', 'Forward')<CR>
"---
xnoremap <silent> ]m :<C-U>call <SID>searchsyn('\<def\>',['rubyDefine'],'','v')<CR>
"---
xnoremap <silent> ]m :<C-U>exe "normal! gv"<Bar>call <SID>DoFeatureMotion(v:count1, 'W')<CR>
"---
xnoremap <silent> aM :<C-U>call <SID>wrap_a('[[','][')<CR>
"---
xnoremap <silent> am :<C-U>call <SID>wrap_a('[m',']M')<CR>
"---
xnoremap <silent> iM :<C-U>call <SID>wrap_i('[[','][')<CR>
"---
xnoremap <silent> im :<C-U>call <SID>wrap_i('[m',']M')<CR>
"---
xnoremap <silent>[" :<C-U>exe "normal! gv"<Bar>call search('\%(^\s*".*\n\)\%(^\s*"\)\@!', "bW")<CR>
"---
xnoremap <silent>[[ :<C-U>exe "normal! gv"<Bar>call search('\%(^#\{1,5\}\s\+\S\\|^\S.*\n^[=-]\+$\)', "bsW")<CR>
"---
xnoremap <silent>[[ m':<C-U>exe "normal! gv"<Bar>call search('^\s*\(fu\%[nction]\\|\(export\s\+\)\?def\)\>', "bW")<CR>
"---
xnoremap <silent>[] m':<C-U>exe "normal! gv"<Bar>call search('^\s*end\(f\%[unction]\\|\(export\s\+\)\?def\)\>', "bW")<CR>
"---
xnoremap <silent>]" :<C-U>exe "normal! gv"<Bar>call search('\%(^\s*".*\n\)\@<!\%(^\s*"\)', "W")<CR>
"---
xnoremap <silent>][ m':<C-U>exe "normal! gv"<Bar>call search('^\s*end\(f\%[unction]\\|\(export\s\+\)\?def\)\>', "W")<CR>
"---
xnoremap <silent>]] :<C-U>exe "normal! gv"<Bar>call search('\%(^#\{1,5\}\s\+\S\\|^\S.*\n^[=-]\+$\)', "sW")<CR>
"---
xnoremap <silent>]] m':<C-U>exe "normal! gv"<Bar>call search('^\s*\(fu\%[nction]\\|\(export\s\+\)\?def\)\>', "W")<CR>
"---
xnoremap K :Evaluate<CR>
"---
nnoremap x f/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x t/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x t/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x t/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x F/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x F/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x F/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x T/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x T/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x T/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f?:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f?:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f?:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x T?:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x T?:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x T?:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x r/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x r/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x r/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x r?:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x r?:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x r?:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x m/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x m/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x m/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x @/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x @/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x @/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x "/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x "/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x "/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x `/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x `/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x `/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x '/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x '/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x '/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x $f/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x $f/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x $f/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x df/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x df/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x df/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x dt/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x dt/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x dt/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x yf/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x yf/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x yf/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x 2f/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x 2f/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x 2f/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f/:so foo<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/:so foo<CR>
" text: one\ntwo\n
"---
nnoremap x f/:so foo<CR>
" text: x/y/z/?\n
"---
nnoremap x f/:nnoremap y :!echo hi<lt>CR><CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/:nnoremap y :!echo hi<lt>CR><CR>
" text: one\ntwo\n
"---
nnoremap x f/:nnoremap y :!echo hi<lt>CR><CR>
" text: x/y/z/?\n
"---
nnoremap x f/f/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/f/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f/f/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f//:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f//:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f//:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f/<Esc>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/<Esc>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f/<Esc>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f/<CR>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/<CR>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f/<CR>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x /a<CR>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x /a<CR>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x /a<CR>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x /a<Esc>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x /a<Esc>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x /a<Esc>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x /a:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x /a:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x /a:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x ?a:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x ?a:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x ?a:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x /a<BS><BS>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x /a<BS><BS>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x /a<BS><BS>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x :s/a/b/<CR>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x :s/a/b/<CR>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x :s/a/b/<CR>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x :g/a/d<CR>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x :g/a/d<CR>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x :g/a/d<CR>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x :g/a/!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x :g/a/!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x :g/a/!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x /a<C-c>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x /a<C-c>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x /a<C-c>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x ?a<C-[>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x ?a<C-[>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x ?a<C-[>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x /a<CR>/b<CR>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x /a<CR>/b<CR>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x /a<CR>/b<CR>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f/f?:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/f?:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f/f?:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x <Cmd>/a<CR>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x <Cmd>/a<CR>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x <Cmd>/a<CR>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x f/<Cmd>!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x f/<Cmd>!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x f/<Cmd>!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x vf/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x vf/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x vf/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x :"/:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x :"/:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x :"/:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x i/<Esc>:!echo hi<CR>
" text: a/b?c\ntwo\n
"---
nnoremap x i/<Esc>:!echo hi<CR>
" text: one\ntwo\n
"---
nnoremap x i/<Esc>:!echo hi<CR>
" text: x/y/z/?\n
"---
nnoremap x :sof<Left><Cmd>m<CR><Right><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :sof<Left><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :so<Home><Cmd>m<CR><End><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :<Cmd>m<CR>so
" text: a/b?c:d\ntwo\n
"---
nnoremap x :1,<Cmd>m<CR>!echo hi
" text: a/b?c:d\ntwo\n
"---
nnoremap x :1,<Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :1<Cmd>m<CR><Left><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :let mapleader=<Cmd>m<CR><Home><Cmd>m<CR><End>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :w<Cmd>m<CR><Left><Cmd>m<CR><Right><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :,<Cmd>m<CR><Left><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :!<Cmd>m<CR><Left><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :!echo hi<Left><Cmd>m<CR><Right><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap x 1,<Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap x <Cmd>m<CR><Left><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap x <Home><Cmd>m<CR>so f
" text: a/b?c:d\ntwo\n
"---
nnoremap x :se sh=<Left><Left><Cmd>m<CR><Right><Right><Cmd>m<CR>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :tabdo tabdo tabdo tabdo tabdo tabdo tabdo tabdo sox so<Left>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :let mapleader=','<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :set langremap<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :set lrm<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :setlocal langremap<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :setglobal langremap<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :setl lrm<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :let mapleader=','<Bar><Cmd>m<CR><C-u>
" text: a/b?c:d\ntwo\n
"---
nnoremap x <Cmd>let mapleader=','<Bar>
" text: a/b?c:d\ntwo\n
"---
vnoremap x :let mapleader=','<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x yi':1set langremap<Home>:
" text: a/b?c:d\ntwo\n
"---
nnoremap x :sof<Left>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :sof<Left><Cmd>m<CR><C-u>
" text: a/b?c:d\ntwo\n
"---
nnoremap x :'so f<Home><Right>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Tab> langmap=q:<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Tab> mapleader=','<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap x ar !echo hi<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Space> ar !echo hi<End><C-k><CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Space> alet mapleader=','<Right><Home><CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Tab> aset langmap=q:<Left><Right><CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Space> a,1let mapleader=','<CR>
" text: a/b?c:d\ntwo\n
"---
cnoremap x let mapleader=','<Bar>
" text: a/b?c:d\ntwo\n
"---
cnoremap x let mapleader=','<Bar><Right>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Space> let mapleader=','<Bar>
" text: a/b?c:d\ntwo\n
"---
cnoremap <Space> <C-u>let mapleader=','<Left><Left><Del>
" text: a/b?c:d\ntwo\n
"---
cnoremap x set langremap<Bar>
" text: a/b?c:d\ntwo\n
"---
nnoremap x d:T<Del>*w:<Left>!echo hi<CR>
"---
nnoremap x :let mapleader=','<Bar>
"---
nnoremap x :setglobal langremap<Bar>
"---
cnoremap x let mapleader=','<Bar>
"---
nnoremap x <Cmd>so f<CR>
"---
nnoremap x ::so f<CR>
"---
nnoremap x :<C-u>!ls<CR>
"---
nnoremap x :ab<Home>!ls<End><CR>
"---
nnoremap x :!ls<C-u>noh<CR>
"---
cnoremap x <C-u>!ls<CR>
"---
cnoremap x <BS>
"---
nnoremap x :g/x/normal dd<CR>
"---
nnoremap x @a
"---
nnoremap x !!ls<CR>
"---
nnoremap x q:
"---
inoremap x <C-r>a
"---
inoremap x <C-o>:!ls<CR>
"---
inoremap x a:b<Esc>:!ls<CR>
"---
nnoremap <expr> x ':!ls<CR>'
"---
nmap x :!ls<CR>
"---
nnoremap x <M-:>!ls<CR>
"---
let mapleader = ":"
nnoremap x <Leader>!ls<CR>
"---
let mapleader = ":"
nnoremap <Leader>x :echo 1<CR>
"---
set langmap=j: langremap
nnoremap x 0j!ls<CR>
"---
set langmap=j:
nnoremap x 0js/hello/X/<CR>
"---
