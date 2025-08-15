import { platform } from "@electron-toolkit/utils";
import cliTruncate from "cli-truncate";
import electron, { BrowserWindow, Menu, MenuItem } from "electron";
const isDev = import.meta.env.DEV;

const webContents = (win: BrowserWindow) => win.webContents ?? (win.id && win);
type DecoratedMenuItem = MenuItem & Partial<{ transform: (text: string) => string; click: (menuItem: MenuItem) => void }>;

const decorateMenuItem =
	<T extends MenuItem>(menuItem: T) =>
	(options: { transform?: (text: string) => string; click?: () => void } = {}): T => {
		if (options.transform && !("transform" in menuItem)) {
			(menuItem as unknown as DecoratedMenuItem).transform = options.transform;
		}

		return menuItem;
	};
const removeUnusedMenuItems = (menuTemplate: DecoratedMenuItem[]) => {
	let notDeletedPreviousElement;

	return menuTemplate
		.filter((menuItem) => menuItem && menuItem.visible)
		.filter((menuItem, index, array) => {
			const toDelete = menuItem.type === "separator" && (!notDeletedPreviousElement || index === array.length - 1 || array[index + 1].type === "separator");
			notDeletedPreviousElement = toDelete ? notDeletedPreviousElement : menuItem;
			return !toDelete;
		});
};

const create = (win: BrowserWindow, options: ContextMenuOptions) => {
	const handleContextMenu = (event, properties) => {
		if (typeof options.shouldShowMenu === "function" && options.shouldShowMenu(event, properties) === false) {
			return;
		}

		const { editFlags } = properties;
		const hasText = properties.selectionText.length > 0;
		const isLink = Boolean(properties.linkURL);
		const can = (type: string) => editFlags[`can${type}`] && hasText;

		const defaultActions = {
			separator: () => ({ type: "separator" }),
			lookUpSelection: decorateMenuItem({
				id: "lookUpSelection",
				label: "Look Up “{selection}”",
				visible: platform.isMacOS && hasText && !isLink,
				click() {
					if (platform.isMacOS) {
						webContents(win).showDefinitionForSelection();
					}
				},
			}),
			searchWithGoogle: decorateMenuItem({
				id: "searchWithGoogle",
				label: "&Search with Google",
				visible: hasText,
				click() {
					const url = new URL("https://www.google.com/search");
					url.searchParams.set("q", properties.selectionText);
					electron.shell.openExternal(url.toString());
				},
			}),
			cut: decorateMenuItem({
				id: "cut",
				label: "Cu&t",
				enabled: can("Cut"),
				visible: properties.isEditable,
				click(menuItem) {
					const target = webContents(win);

					if (!menuItem.transform && target) {
						target.cut();
					} else {
						properties.selectionText = menuItem.transform ? menuItem.transform(properties.selectionText) : properties.selectionText;
						electron.clipboard.writeText(properties.selectionText);
					}
				},
			}),
			copy: decorateMenuItem({
				id: "copy",
				label: "&Copy",
				enabled: can("Copy"),
				visible: properties.isEditable || hasText,
				click(menuItem) {
					const target = webContents(win);

					if (!menuItem.transform && target) {
						target.copy();
					} else {
						properties.selectionText = menuItem.transform ? menuItem.transform(properties.selectionText) : properties.selectionText;
						electron.clipboard.writeText(properties.selectionText);
					}
				},
			}),
			paste: decorateMenuItem({
				id: "paste",
				label: "&Paste",
				enabled: editFlags.canPaste,
				visible: properties.isEditable,
				click(menuItem) {
					const target = webContents(win);

					if (menuItem.transform) {
						let clipboardContent = electron.clipboard.readText(properties.selectionText);
						clipboardContent = menuItem.transform ? menuItem.transform(clipboardContent) : clipboardContent;
						target.insertText(clipboardContent);
					} else {
						target.paste();
					}
				},
			}),
			selectAll: decorateMenuItem({
				id: "selectAll",
				label: "Select &All",
				click() {
					webContents(win).selectAll();
				},
			}),
		};

		const shouldShowSelectAll = options.showSelectAll || (options.showSelectAll !== false && platform.isMacOS);

		let menuTemplate = [
			defaultActions.separator(),
			options.showLookUpSelection !== false && defaultActions.lookUpSelection(),
			defaultActions.separator(),
			options.showSearchWithGoogle !== false && defaultActions.searchWithGoogle(),
			defaultActions.separator(),
			defaultActions.cut(),
			defaultActions.copy(),
			defaultActions.paste(),
			shouldShowSelectAll && defaultActions.selectAll(),
		];

		if (options.menu) {
			menuTemplate = options.menu(defaultActions, properties, win, dictionarySuggestions, event);
		}

		if (options.prepend) {
			const result = options.prepend(defaultActions, properties, win, event);

			if (Array.isArray(result)) {
				menuTemplate.unshift(...result);
			}
		}

		if (options.append) {
			const result = options.append(defaultActions, properties, win, event);

			if (Array.isArray(result)) {
				menuTemplate.push(...result);
			}
		}

		// Filter out leading/trailing separators
		// TODO: https://github.com/electron/electron/issues/5869
		menuTemplate = removeUnusedMenuItems(menuTemplate);

		for (const menuItem of menuTemplate) {
			// Apply custom labels for default menu items
			if (options.labels && options.labels[menuItem.id]) {
				menuItem.label = options.labels[menuItem.id];
			}

			// Replace placeholders in menu item labels
			if (typeof menuItem.label === "string" && menuItem.label.includes("{selection}")) {
				const selectionString = typeof properties.selectionText === "string" ? properties.selectionText.trim() : "";
				menuItem.label = menuItem.label.replace("{selection}", cliTruncate(selectionString, 25).replaceAll("&", "&&"));
			}
		}

		if (menuTemplate.length > 0) {
			const menu = electron.Menu.buildFromTemplate(menuTemplate);

			if (typeof options.onShow === "function") {
				menu.on("menu-will-show", options.onShow);
			}

			if (typeof options.onClose === "function") {
				menu.on("menu-will-close", options.onClose);
			}

			menu.popup(win);
		}
	};

	webContents(win).on("context-menu", handleContextMenu);

	return () => {
		if (win?.isDestroyed?.()) {
			return;
		}

		webContents(win).removeListener("context-menu", handleContextMenu);
	};
};
type ContextMenuOptions = {
	window?: BrowserWindow;
	shouldShowMenu?: (event: Event, properties: any) => boolean;
	showInspectElement?: boolean;
	showSelectAll?: boolean;
	showLearnSpelling?: boolean;
	showLookUpSelection?: boolean;
	showSearchWithGoogle?: boolean;
	showSaveImage?: boolean;
	showSaveImageAs?: boolean;
	showCopyImage?: boolean;
	showCopyImageAddress?: boolean;
	showSaveVideo?: boolean;
	showSaveVideoAs?: boolean;
	showCopyVideoAddress?: boolean;
	showCopyLink?: boolean;
	showSaveLinkAs?: boolean;
	showServices?: boolean;
	menu?: (defaultActions: any, properties: any, win: BrowserWindow, dictionarySuggestions: MenuItem[], event: Event) => MenuItem[];
	prepend?: (defaultActions: any, properties: any, win: BrowserWindow, event: Event) => MenuItem[];
	append?: (defaultActions: any, properties: any, win: BrowserWindow, event: Event) => MenuItem[];
	labels?: Record<string, string>;
	onShow?: (event: Event, menu: Menu) => void;
	onClose?: (event: Event, menu: Menu) => void;
};
export default function contextMenu(options: ContextMenuOptions = {}) {
	if (process.type === "renderer") {
		throw new Error("Cannot use electron-context-menu in the renderer process!");
	}

	let isDisposed = false;
	const disposables: (() => void)[] = [];

	const init = (win: BrowserWindow) => {
		if (isDisposed) {
			return;
		}

		const disposeMenu = create(win, options);

		const disposable = () => {
			disposeMenu();
		};

		webContents(win).once("destroyed", disposable);
	};

	const dispose = () => {
		for (const dispose of disposables) {
			dispose();
		}

		disposables.length = 0;
		isDisposed = true;
	};

	if (options.window) {
		const win = options.window;

		// When window is a webview that has not yet finished loading webContents is not available
		if (webContents(win) === undefined) {
			const onDomReady = () => {
				init(win);
			};

			win.webContents.once("dom-ready", onDomReady);

			disposables.push(() => {
				win.webContents.off("dom-ready", onDomReady);
			});

			return dispose;
		}

		init(win);

		return dispose;
	}

	for (const win of electron.BrowserWindow.getAllWindows()) {
		init(win);
	}

	const onWindowCreated = (event, win) => {
		init(win);
	};

	electron.app.on("browser-window-created", onWindowCreated);
	disposables.push(() => {
		electron.app.removeListener("browser-window-created", onWindowCreated);
	});

	return dispose;
}
