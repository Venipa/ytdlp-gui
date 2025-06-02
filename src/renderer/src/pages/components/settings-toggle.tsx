import { SwitchButton } from "@renderer/components/ui/switch-button";
import { get } from "lodash-es";
import { PropsWithChildren, useMemo } from "react";
import { useApp } from "./app-context";

type SettingsToggleProps = PropsWithChildren<{
	name: string;
	onChange?: (value: boolean) => void;
	disabled?: boolean;
}>;
export default function SettingsToggle({ name: key, children, ...props }: SettingsToggleProps) {
	const { settings, setSetting } = useApp();
	const settingsValue = useMemo(() => get(settings, key), [key, settings]);
	return (
		<SwitchButton
			className='flex items-center text-sm'
			defaultChecked={settingsValue}
			checked={settingsValue}
			onCheckedChange={() =>
				setSetting(key, !settingsValue, true).then(({ value: newValue }) => {
					props.onChange?.(newValue);
					return newValue;
				})
			}
			disabled={props.disabled}>
			{children}
		</SwitchButton>
	);
}
