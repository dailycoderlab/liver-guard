import { Copy, Download, FilePlus2, Pencil, Trash2, Upload } from 'lucide-react';
import { useEffect } from 'react';
import { cn } from '@/lib/cn';
import { createScript, deleteScript, duplicateScript, exportScript, importScript, openScript, refreshScripts, renameScript } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { Button, Empty } from './ui';

export function ScriptsPanel() {
  const scripts = useUi((s) => s.scripts);
  const current = useEditor((s) => s.scriptId);
  useEffect(() => void refreshScripts(), []);

  return (
    <div className="flex h-full flex-col">
      <div className="flex gap-1.5 p-3">
        <Button className="min-w-0 flex-1" variant="primary" onClick={createScript}>
          <FilePlus2 size={14} /> 新建脚本
        </Button>
        <Button onClick={importScript} title="从文件导入脚本（含模板图片），作为新脚本添加">
          <Upload size={14} /> 导入
        </Button>
      </div>
      {scripts.length === 0 ? (
        <Empty>还没有脚本</Empty>
      ) : (
        <ul className="flex-1 overflow-y-auto px-2 pb-2">
          {scripts.map((s) => (
            <li
              key={s.id}
              onClick={() => openScript(s.id)}
              className={cn('group flex cursor-pointer items-center gap-1 rounded-md px-2 py-1.5', s.id === current ? 'bg-accent/20 text-white' : 'hover:bg-raised')}
            >
              <span className="min-w-0 flex-1 truncate">{s.name}</span>
              <button
                className="hidden text-mute hover:text-ink group-hover:block"
                title="重命名"
                onClick={(e) => {
                  e.stopPropagation();
                  void renameScript(s.id, s.name);
                }}
              >
                <Pencil size={13} />
              </button>
              <button
                className="hidden text-mute hover:text-ink group-hover:block"
                title="导出为文件"
                onClick={(e) => {
                  e.stopPropagation();
                  void exportScript(s.id);
                }}
              >
                <Download size={13} />
              </button>
              <button
                className="hidden text-mute hover:text-ink group-hover:block"
                title="复制"
                onClick={(e) => {
                  e.stopPropagation();
                  void duplicateScript(s.id, s.name);
                }}
              >
                <Copy size={13} />
              </button>
              <button
                className="hidden text-mute hover:text-red-400 group-hover:block"
                title="删除"
                onClick={(e) => {
                  e.stopPropagation();
                  void deleteScript(s.id, s.name);
                }}
              >
                <Trash2 size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
