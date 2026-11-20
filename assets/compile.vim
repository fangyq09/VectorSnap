"=============================================================================
" 	     File: compile.vim
"      Author: Yangqin Fang
"       Email: fangyq09@gmail.com
"  Description: A compile plugin for LaTeX
"  In normal mode:
"  1. Press <F2> to run pdflatex or xelatex (auto detect TeX engine); 
"  2. Press <F5> to run pdflatex; 
"  3. Press <F6> to run xelatex;
"  4. Press <F8> to compile bibtex or biblatex;
"  In case you split your project into many separated tex files, for example 
"  chapter1.tex, chapter2.tex, ..., in any chapter, the shortcuts are all feasible.
"=============================================================================
if exists('b:loaded_vim_latex_compile')
	finish
endif
let b:loaded_vim_latex_compile = 1

if has("win32") || has("win64")
	set shellslash
	let s:tex_cmd_term = ['cmd.exe', '/c']
	let b:pdf_viewer = 'VectorSnap'
	let b:Path_VectorSnap = expand('C:\Tools\VectorSnap.exe')
	let b:Path_SumatraPDF = expand($LOCALAPPDATA . '/SumatraPDF/SumatraPDF.exe')
	let s:cmd_cd = 'cd /d'
elseif has("unix")
	let s:tex_cmd_term = ['/bin/sh', '-c']
	let b:pdf_viewer = 'VectorSnap'
	let b:Path_VectorSnap = expand('~/.local/bin/VectorSnap')
	let s:cmd_cd = 'cd'
endif 

let s:tex_efm = '%-P**%f,%E%f:%l: %m,%I%f:%l: %m,%Z<argument> %m,%Cl.%l %m,%-G%.%#'
let b:tex_engine_options = '-synctex=1 -file-line-error -interaction=nonstopmode'
let b:tex_compile_output = []
let b:tex_out_dir = 'build'


function! s:IsMainOf(f, pat_class, pat_input, ...)"{{{
	let limit = a:0 ? a:1 : 0
	let has_class = 0
	let has_input = 0
	let n = 0
	for line in readfile(a:f)
		let n += 1
		if !has_class && (limit <= 0 || n <= limit) && line =~# a:pat_class
			let has_class = 1
		endif
		if !has_input && line =~# a:pat_input
			let has_input = 1
		endif
		if has_class && has_input
			return 1                " 两个都命中，提前结束
		endif
		if limit > 0 && n == limit && !has_class
			return 0                " 限制范围内没有 documentclass，不可能是主文件
		endif
	endfor
	return 0
endfunction
"}}}

function! s:FindMainCandidates(path, file, ...) "{{{
	let pat_class = '^\s*\\documentclass'
	"let pat_input = '^\s*\\\(input\|include\){\s*'.escape(a:file, '.\').'\s*}'
	" 改为：兼容带或不带 .tex 后缀（如 chapter1.tex 或 chapter1）
	let l:file_no_ext = fnamemodify(a:file, ':r')
	let l:file_pat = escape(a:file, '.\')
	if l:file_no_ext !=# a:file && !empty(l:file_no_ext)
		let l:file_pat = '\(' . escape(a:file, '.\') . '\|' . escape(l:file_no_ext, '.\') . '\)'
	endif
	let pat_input = '^\s*\\\(input\|include\){\s*' . l:file_pat . '\s*}'
	let result = []
	for f in glob(a:path.'/*.tex', 0, 1)
		if call('s:IsMainOf', [f, pat_class, pat_input] + a:000)
			call add(result, fnamemodify(f, ':p'))
		endif
	endfor
	return result
endfunction
"}}}

function! s:tex_Prompt(path,file)"{{{
	let mtf_common = s:FindMainCandidates(a:path, a:file)
	if len(mtf_common)==1
		call add(mtf_common,a:path)
		return mtf_common
	elseif len(mtf_common)>1
		let output = map(copy(mtf_common),'fnamemodify(v:val, ":t")')
		let num_output = []
		for ii in range(len(output))
			call add(num_output,string(ii+1).'.'.output[ii])
		endfor
		call inputsave()
		let file_choose = inputdialog("Please choose main tex file [".join(num_output,'; ')."]: ",1,0)
		call inputrestore()
		if (file_choose < 1) || (file_choose > len(output))
			return ['','']
		else
			let file_name = mtf_common[file_choose-1]
			return [file_name, a:path]
		endif
	else
		return ['','']
	endif
endfunction
"}}}

function! s:Find_Main_TeX_File() "{{{
		let save_cursor = getpos(".")
		let view = winsaveview()
		call cursor(1, 1)
		if search('^\s*\\documentclass','cnw')
			let main_tex_file = expand('%:p')
			let main_tex_dir = expand('%:p:h')
			let main_tex = [main_tex_file,main_tex_dir]
		else
			let cur_dir = expand('%:p:h')
			let cur_file_name = expand('%:p')
			let par_dir = expand('%:p:h:h')
			let tex_file = expand('%:p:t')
			let main_tex = s:tex_Prompt(cur_dir,tex_file)
			"if main_tex[0] == ''
			"	let file_name_keep = substitute(cur_file_name,par_dir.'/','','')
			"	let main_tex = s:tex_Prompt(par_dir,file_name_keep)
			"endif
			if empty(main_tex[0])
				let l:par_prefix = par_dir . '/'
				if stridx(cur_file_name, l:par_prefix) == 0
					let file_name_keep = strpart(cur_file_name, strlen(l:par_prefix))
				else
					let file_name_keep = tex_file
				endif
				let main_tex = s:tex_Prompt(par_dir, file_name_keep)
			endif
		endif
		call setpos('.', save_cursor)
		call winrestview(view)
	return main_tex
endfunction
"}}}

function! s:VectorSnap_SynctexForward(pdf_file,source,line,col,build_dir)"{{{
    let l:out_dir = trim(a:build_dir)

    " 获取 PDF 阅读器程序
    if exists('b:Path_VectorSnap')
        let l:pdf_viewer_prog = b:Path_VectorSnap
    else
        let l:pdf_viewer_prog = 'VectorSnap'
    endif

    " 获取当前 Vim ServerName
    let l:server_name = v:servername

		" 获取当前 Vim/Neovim 可执行文件
		let l:editor = get(g:, 'neovide', v:false) ? 'neovide' : v:progname


    " 构造启动命令
    let l:execstr = shellescape(l:pdf_viewer_prog) . ' ' . shellescape(a:pdf_file)

		if !empty(l:out_dir)
				let l:dir = fnamemodify(a:pdf_file, ':h')
				let l:target_dir = l:dir . '/' . l:out_dir 
				let synctex_dir = '|' . l:target_dir
		else
			let synctex_dir = ''
		endif

		let l:synctex_arg = '--synctex=' . a:line . ':' . a:col . ':' . a:source . l:synctex_dir
		let l:execstr .= ' ' . shellescape(l:synctex_arg)

    if !empty(l:server_name)
        let l:execstr .= ' --servername=' . shellescape(l:server_name)
    endif

    if !empty(l:editor)
        let l:execstr .= ' --editor=' . shellescape(l:editor)
    endif
		if has('win32')
			silent execute '!start "" /b ' . l:execstr
		else
			"call system(l:execstr . ' &')
			call system(l:execstr . ' >/dev/null 2>&1 &')
		endif
endfunction
"}}}

function! s:SumatraSynctexForward(pdf,source,line,col,...)"{{{
	if !exists('b:Path_SumatraPDF') || (b:Path_SumatraPDF == '')
		if !exists('g:Path_SumatraPDF')
			if executable('SumatraPDF')
				let b:Path_SumatraPDF = 'SumatraPDF'
			elseif executable($LOCALAPPDATA . '/SumatraPDF/SumatraPDF.exe')
				let b:Path_SumatraPDF = $LOCALAPPDATA . '/SumatraPDF/SumatraPDF.exe'
			elseif executable('C:/Program Files/SumatraPDF/SumatraPDF.exe')
				let b:Path_SumatraPDF = 'C:/Program Files/SumatraPDF/SumatraPDF.exe'
			else
				let b:Path_SumatraPDF = ''
				echom 'Can not find SumatraPDF!'
			endif
		else
			let b:Path_SumatraPDF = g:Path_SumatraPDF
		endif
	endif
	if b:Path_SumatraPDF != ''
		silent execute '!start '.fnameescape(b:Path_SumatraPDF).' -reuse-instance '
					\ .fnameescape(a:pdf).' -forward-search "'.a:source.'" '.a:line
	endif
endfunction

function! s:ZathuraSynctexForward(pdf,source,line,col,...)"{{{
	let l:target_pdf = a:pdf
	let input = shellescape(a:line . ":" . a:col . ":" . a:source)
	if has("nvim")
		let base_cmd =
					\ 'if [ -S ' . v:servername . ' ]; then ' .
					\ 'nvim --server ' . v:servername .
					\ ' --remote-expr "call(''tex#outils#JumpToLine'', [''%{input}'', %{line}])"; ' .
					\ 'else setsid neovide "%{input}" -- ' .
					\ '--listen ' . v:servername .
					\ ' +":call tex#outils#OpenTexAndJump(''%{input}'', %{line})" >/dev/null 2>&1 & fi'

		let cmd = 'sh -c ' . shellescape(base_cmd)
		let execstr = 'zathura -x ' . shellescape(cmd) . ' ' .
					\ '--synctex-forward=' . input . ' ' . fnameescape(l:target_pdf) . ' &'
	else
		let execstr = 'zathura --synctex-forward=' . input . ' '
					\ . fnameescape(l:target_pdf) . ' &'
	endif
  silent call system(execstr)
endfunction
"}}}
"}}}

function! s:ZathuraSynctexForward(pdf,source,line,col,...)"{{{
	let l:target_pdf = a:pdf
	let input = shellescape(a:line . ":" . a:col . ":" . a:source)
	if has("nvim")
		let base_cmd =
					\ 'if [ -S ' . v:servername . ' ]; then ' .
					\ 'nvim --server ' . v:servername .
					\ ' --remote-expr "call(''tex#outils#JumpToLine'', [''%{input}'', %{line}])"; ' .
					\ 'else setsid neovide "%{input}" -- ' .
					\ '--listen ' . v:servername .
					\ ' +":call tex#outils#OpenTexAndJump(''%{input}'', %{line})" >/dev/null 2>&1 & fi'

		let cmd = 'sh -c ' . shellescape(base_cmd)
		let execstr = 'zathura -x ' . shellescape(cmd) . ' ' .
					\ '--synctex-forward=' . input . ' ' . fnameescape(l:target_pdf) . ' &'
	else
		let execstr = 'zathura --synctex-forward=' . input . ' '
					\ . fnameescape(l:target_pdf) . ' &'
	endif
  silent call system(execstr)
endfunction
"}}}

function! s:TeXViewPDF(pdf_file,source,line,col,build_dir)"{{{
	if b:pdf_viewer ==? 'Zathura'
		call <SID>ZathuraSynctexForward(a:pdf_file,a:source,a:line,a:col,a:build_dir)
	elseif b:pdf_viewer ==? 'VectorSnap'
		call <SID>VectorSnap_SynctexForward(a:pdf_file,a:source,a:line,a:col,a:build_dir)
	elseif b:pdf_viewer ==? 'Sumatra'
		call <SID>SumatraSynctexForward(a:pdf_file,a:source,a:line,a:col,a:build_dir)
	endif
endfunction
"}}}

function! s:CopyFile(src, dst) "{{{
	if !filereadable(a:src)
		return
	endif
	if exists('*filecopy')
		call filecopy(a:src, a:dst)
	else
		call writefile(readfile(a:src, 'B'), a:dst, 'B')
	endif
endfunction
""}}}

"{{{function! s:TeXCompileCloseHandler
function! s:TeXCompileCloseHandler(view,proj_dir,pdf_file,source,line,col,out_dir,...) 	
	let l:save_cwd = getcwd()
	try
		execute 'cd ' . fnameescape(a:proj_dir)
		call setqflist([], 'r', { 'lines': b:tex_compile_output, 'efm': s:tex_efm})
	finally
		execute 'cd ' . fnameescape(l:save_cwd)
	endtry
	let b:tex_compile_output = [] " 清空缓存

	let l:qflist = getqflist()
	if empty(l:qflist)
		cclose
		echom "successfully compiled"
		let l:out_dir = trim(a:out_dir)
		if l:out_dir !=# ''
			let l:pdf_name = fnamemodify(a:pdf_file, ':t')
			let l:src_pdf = fnamemodify(a:proj_dir . '/' . a:out_dir . '/' . l:pdf_name, ':p')
			let l:dst_pdf = fnamemodify(a:pdf_file, ':p')

			"let l:src_synctex = fnamemodify(l:src_pdf, ':r') . '.synctex.gz'
			"let l:dst_synctex = fnamemodify(a:pdf_file, ':r') . '.synctex.gz'
			"if filereadable(l:src_synctex)
			"	call s:CopyFile(l:src_synctex, l:dst_synctex)
			"endif
			if filereadable(l:src_pdf)
				call s:CopyFile(l:src_pdf, l:dst_pdf)
			endif
		endif
		if a:view
			silent! call <SID>TeXViewPDF(a:pdf_file,a:source,a:line,a:col,a:out_dir)
		endif
	else 
		copen 5      " open quickfix window
		wincmd p    " jump back to previous window
		echohl WarningMsg
		echomsg "compile failed with errors"
		echohl None
	endif
endfunction
"}}}
function! s:TeXCompileOutHandler(job_id, msg,...)"{{{OutHandler
	if type(a:msg) == v:t_list
		call extend(b:tex_compile_output, a:msg)
	else
		call add(b:tex_compile_output, a:msg)
	endif
endfunction
"}}}

"{{{ RunLaTeX_job(...) 
function! RunLaTeX_job(tex_file,proj_dir,engine,view,track_source,track_line,track_col, out_dir)
	let pdf_file = fnamemodify(a:tex_file,':p:r').'.pdf'
	"切换编译时候的工作目录，错误转跳的时候需要用到
	execute 'silent! lcd ' . fnameescape(a:proj_dir) 
	let l:out_prm = ''
	let l:out_dir = trim(a:out_dir)
	if l:out_dir !=# ''
			let l:out_prm = ' -output-directory="' . l:out_dir . '"'
	endif
	let tex_cmd_opt = s:cmd_cd . ' ' . fnameescape(a:proj_dir) . ' && '
				\ . a:engine . ' ' . b:tex_engine_options . l:out_prm . ' ' 
				\ . fnameescape(a:tex_file)
	let cmd = s:tex_cmd_term + [tex_cmd_opt]
	call setqflist([])
	if has("nvim")
		let job_options = {
					\ 'cwd': a:proj_dir,
					\ 'on_stdout': function('s:TeXCompileOutHandler'),
					\ 'on_exit': function('s:TeXCompileCloseHandler', 
					\ [a:view,a:proj_dir,pdf_file,a:track_source,a:track_line,a:track_col,a:out_dir]),
					\ }
		let b:run_tex_job = jobstart(cmd, job_options)
	else
		let job_options = {
					\ 'cwd': a:proj_dir,
					\ 'out_io': 'pipe',
					\ 'out_cb': function('s:TeXCompileOutHandler'),
					\ 'close_cb': function('s:TeXCompileCloseHandler',
					\ [a:view,a:proj_dir,pdf_file,a:track_source,a:track_line,a:track_col,a:out_dir]),
					\ }
		let b:run_tex_job = job_start(cmd, job_options)
	endif
endfunction
"}}}

"{{{ Find TeX engine
function! s:Find_TeX_engine() 
	" Get the TeX engine from the line % !TeX engine/grogram = pdflatex/xelatex
	"let com_str = '^\c\s*%\+.\{-}\(!\)*\s*\(TeX\)*\s*\(engine\|program\)*\s*\(=\|:\)*\s*\(pdf\|xe\|lau\)*latex.*'
	let l:current_file = expand('%:p')
	let com_str = '^\c\s*%.\{-}\(pdf\|xe\|lua\)latex\(\s.*\)*$' 
	let save_cursor = getpos(".")
	if b:tex_main_file_name ==# l:current_file
		call cursor(1, 1)
		if !exists('b:doc_class_line')
			let b:doc_class_line = search('^\s*\\documentclass','cw')
		endif
		if !exists('b:doc_begin_doc')
			let b:doc_begin_doc = search('^\s*\\begin\s*{\s*document\s*}','cnw')
		endif
		call cursor(1, 1)
		let tex_engine_com_line = search(com_str,'c',b:doc_class_line)
		let xelatex_marker = search('^\s*\\\(setmainfont\|setCJKmainfont\)','cn',b:doc_begin_doc)
		if tex_engine_com_line
			let line_text = getline(tex_engine_com_line)
		endif
	else
		let main_tex_file_text = readfile(b:tex_main_file_name)
		let line_num = 0
		let tex_engine_com_line = 0
		let xelatex_marker = 0
		for item in main_tex_file_text
			let line_num = line_num + 1 
			if item =~ com_str
				let line_text = item 
				let tex_engine_com_line = line_num
				break
			endif
			if item =~ '^\s*\\\(setmainfont\|setCJKmainfont\)'
				let xelatex_marker = line_num
				break
			endif
		endfor
	endif
	if (tex_engine_com_line == 0)&&(xelatex_marker == 0)
		let tex_engine = 'pdflatex'
	elseif tex_engine_com_line > 0
		let tex_engine_pre = matchstr(line_text, '\c\(pdf\|xe\|lua\)latex')
		let tex_engine = tolower(tex_engine_pre)
	else
		let tex_engine = 'xelatex'
	endif
	call setpos('.', save_cursor)
	return tex_engine
endfunction
"}}}

"{{{ call RunLaTeX with proper TeX engine
function! s:Compile_LaTeX_Run(engine,view,out_dir) 
	silent write
	if &ft != 'tex'
		echomsg "calling RunLaTeX from a non-tex file"
		return ''
	endif

	let save_cursor = getpos(".")
	let track_line = line(".")
	let track_col = col(".")
	let track_source = expand("%:p")

	if !exists('b:tex_main_file_name')  || !exists('b:tex_proj_dir') || (b:tex_main_file_name == '')
		let [b:tex_main_file_name,b:tex_proj_dir] = s:Find_Main_TeX_File()
	endif
	if b:tex_main_file_name == ''
		echohl WarningMsg
		echomsg "No main tex file!"
		echohl None
		return ''
	endif

	"输出文件夹
	let l:clean_out_dir = trim(a:out_dir)
	if l:clean_out_dir !=# ''
		let l:full_out_path = b:tex_proj_dir . '/' . l:clean_out_dir
		if getftype(l:full_out_path) !=# 'dir'
			call mkdir(l:full_out_path, 'p')
		endif
	endif

	" choose proper TeX engine
	if a:engine == 'auto'
		if !exists('b:tex_engine')
			let b:tex_engine = s:Find_TeX_engine()
		endif
	else 
		let b:tex_engine = a:engine
	endif

	let pdf_file = fnamemodify(b:tex_main_file_name,':p:r').'.pdf'


	""compile latex 
	let b:tex_compile_output = []
	echomsg "compiling with ".b:tex_engine."..."
	if has("nvim")
		silent! call RunLaTeX_job(
					\ b:tex_main_file_name,
					\ b:tex_proj_dir,
					\ b:tex_engine,
					\ a:view,
					\ track_source,
					\ track_line,
					\ track_col,
					\ a:out_dir
					\ )
	else
		silent! call RunLaTeX_job(
					\ b:tex_main_file_name,
					\ b:tex_proj_dir,
					\ b:tex_engine,
					\ a:view,
					\ track_source,
					\ track_line,
					\ track_col,
					\ a:out_dir
					\ )
	endif
	call setpos('.', save_cursor)
endfunction
"}}}

"
function! s:CompileBibTeX(out_dir) "{{{
	if !exists('b:tex_main_file_name') || !exists('b:tex_proj_dir') || empty(b:tex_main_file_name)
		let [b:tex_main_file_name, b:tex_proj_dir] = s:Find_Main_TeX_File()
	endif
	if empty(b:tex_main_file_name)
		echohl WarningMsg
		echomsg "no main tex file!"
		echohl None
		return
	endif

	let l:save_cwd = getcwd()
	try
		execute 'lcd ' . fnameescape(b:tex_proj_dir)
		let l:main_stem = fnamemodify(b:tex_main_file_name, ':t:r')
		let l:main_lines = filereadable(b:tex_main_file_name) ? readfile(b:tex_main_file_name) : []

		if !exists('b:tex_bib_engine') || empty(b:tex_bib_engine)
			let l:has_biblatex = !empty(filter(copy(l:main_lines), {_, line -> line =~# '\\addbibresource\s*{.\+}'}))
			let b:tex_bib_engine = l:has_biblatex ? 'biber' : 'bibtex'
		endif

		let l:clean_out_dir = trim(a:out_dir)
		if !empty(l:clean_out_dir)
			if b:tex_bib_engine ==# 'biber'
				let l:bib_cmd = '!' . b:tex_bib_engine . ' --output-directory ' . shellescape(l:clean_out_dir) . ' ' . shellescape(l:main_stem)
			else
				let l:bib_cmd = '!' . b:tex_bib_engine . ' ' . shellescape(l:clean_out_dir . '/' . l:main_stem)
			endif
		else
			let l:bib_cmd = '!' . b:tex_bib_engine . ' ' . shellescape(l:main_stem)
		endif

		silent! execute l:bib_cmd
		echomsg toupper(b:tex_bib_engine) . ' compiled!'
	finally
		execute 'lcd ' . fnameescape(l:save_cwd)
	endtry
endfunction
"}}}

nnoremap <silent> <buffer><F2> 
			\ :call <SID>Compile_LaTeX_Run('auto',1,b:tex_out_dir)<CR>
nnoremap <silent> <buffer><F5> 
			\ :call <SID>Compile_LaTeX_Run('pdflatex',1,b:tex_out_dir)<CR>
nnoremap <silent> <buffer><F6> 
			\ :call <SID>Compile_LaTeX_Run("xelatex",1,b:tex_out_dir)<CR> 
nnoremap <silent> <buffer><F8> 
			\ :call <SID>CompileBibTeX(b:tex_out_dir)<CR>
"
" vim:fdm=marker:noet:ff=unix
