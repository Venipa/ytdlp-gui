import { LucideCog } from "lucide-react";

export const meta = {
	title: "Settings",
	icon: LucideCog,
	index: 10,
	show: true,
	onClick: (ev: any) => {
		ev.preventDefault();
		return "settings";
	},
};
export default function SettingsTab() {
	return null;
}
