import { Menu } from "electron";

export function createContextMenu() {
	const InputMenu = Menu.buildFromTemplate([
		{
			label: "Undo",
			role: "undo",
		},
		{
			label: "Redo",
			role: "redo",
		},
		{
			type: "separator",
		},
		{
			label: "Cut",
			role: "cut",
		},
		{
			label: "Copy",
			role: "copy",
		},
		{
			label: "Paste",
			role: "paste",
		},
		{
			type: "separator",
		},
		{
			label: "Select all",
			role: "selectAll",
		},
	]);

	document.body.addEventListener("contextmenu", (e) => {
		e.preventDefault();
		e.stopPropagation();

		let node = e.target;

		while (node) {
			if (node instanceof HTMLInputElement || node instanceof HTMLTextAreaElement) {
				InputMenu.popup(remote.getCurrentWindow());
				break;
			}
			node = node.parentNode;
		}
	});
}
