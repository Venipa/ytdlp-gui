#!/bin/node

const LCL = require('last-commit-log')
const { resolve } = require('path')
const { writeFileSync } = require('fs')
console.log("current working dir:", resolve("."))
const lcl = new LCL()
try {
  const lastCommit = lcl.getLastCommitSync()
  if (!lastCommit) throw new Error("No last commit found")
  writeFileSync(resolve(__dirname, 'git.json'), JSON.stringify(lastCommit))
} catch (e) {}
