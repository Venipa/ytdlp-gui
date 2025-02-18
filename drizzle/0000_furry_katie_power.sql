CREATE TABLE `downloads` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`title` text NOT NULL,
	`state` text,
	`filepath` text NOT NULL,
	`filesize` integer,
	`type` text,
	`url` text NOT NULL,
	`source` text NOT NULL,
	`retryCount` integer,
	`error` blob DEFAULT 'null',
	`meta` blob DEFAULT 'null',
	`metaId` text NOT NULL,
	`created` text DEFAULT (current_timestamp) NOT NULL
);
