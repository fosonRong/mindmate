/**
 * ko-KR 词条表：{ 中文原文: 目标语言译文 }
 *
 * 中文原文即 key（zh-CN 目录为空，回退即原文），因此在补齐翻译前，
 * 界面会安全回退展示中文，不会出现空白或 key 泄漏。
 * 完整性由 scripts/i18n_test.py 校验（key 对齐 + 无残留硬编码）。
 */
export default {} as Record<string, string>
