module.exports = async function linkIssuesToPr({ github, context, core }) {
  const title = process.env.TITLE;
  const prNumber = context.payload.pull_request.number;

  // Extract issue numbers from the bracket prefix
  const bracket = title.match(/^\[([^\]]+)\]/);
  if (!bracket) return;

  const issueNumbers = [...bracket[1].matchAll(/#(\d+)/g)].map(m => Number.parseInt(m[1], 10));
  console.log(`Found issues: ${issueNumbers.join(", ")}`);

  // Verify issues exist
  for (const num of issueNumbers) {
    try {
      const { data: issue } = await github.rest.issues.get({
        owner: context.repo.owner,
        repo: context.repo.repo,
        issue_number: num,
      });
      // Make sure it's actually an issue, not a PR
      if (issue.pull_request) {
        core.setFailed(`#${num} is a pull request, not an issue.`);
        return;
      }
    } catch (error) {
      if (error.status === 404) {
        core.setFailed(`Issue #${num} does not exist.`);
        return;
      }
      throw error;
    }
  }

  // Closing keyword approach: update the PR body to include "Closes #N" lines
  const currentBody = context.payload.pull_request.body || "";
  const closingLines = issueNumbers
    .map(n => `Closes #${n}`)
    .filter(line => !currentBody.includes(line));

  if (closingLines.length > 0) {
    const separator = currentBody.length > 0 ? "\n\n" : "";
    const newBody = closingLines.join("\n") + separator + currentBody;

    await github.rest.pulls.update({
      owner: context.repo.owner,
      repo: context.repo.repo,
      pull_number: prNumber,
      body: newBody,
    });

    console.log(`Added to PR body: ${closingLines.join(", ")}`);
  } else {
    console.log("All issues already linked in PR body.");
  }
};
