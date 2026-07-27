interface ToolbarProps {
  onOpen: () => void;
  onSave: () => void;
  hasDocument: boolean;
}

function Toolbar({ onOpen, onSave, hasDocument }: ToolbarProps) {
  return (
    <>
      <button className="toolbar-btn" onClick={onOpen}>
        Open File
      </button>
      <button className="toolbar-btn" disabled={!hasDocument} onClick={onSave}>
        Save
      </button>
    </>
  );
}

export default Toolbar;
